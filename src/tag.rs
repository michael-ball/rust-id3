extern crate std;
extern crate audiotag;

use std::io::{File, SeekSet, SeekCur};
use std::collections::HashMap;

use self::audiotag::{AudioTag, TagError, TagResult, InvalidInputError, UnsupportedFeatureError};

use frame::{Frame, encoding, PictureContent, CommentContent, TextContent, ExtendedTextContent, LyricsContent};
use picture::{Picture, picture_type};
use util;

/// An ID3 tag containing metadata frames. 
pub struct ID3Tag {
    /// The name of the path from which the tags were loaded.
    path: Option<Path>,
    /// The version of the tag. The first byte represents the major version number, while the
    /// second byte represents the revision number.
    version: [u8, ..2],
    /// The size of the tag when read from a file.
    size: u32,
    /// The file offset of the last frame. 
    offset: u64,
    /// The file offset of the first modified frame.
    modified_offset: u64,
    /// The ID3 header flags.
    flags: TagFlags,
    /// A vector of frames included in the tag.
    frames: Vec<Frame>,
    /// A flag used to indicate if a rewrite is needed.
    rewrite: bool
}

/// Flags used in the ID3 header.
pub struct TagFlags {
    /// Indicates whether or not unsynchronization is used.
    pub unsynchronization: bool,
    /// Indicates whether or not the header is followed by an extended header.
    pub extended_header: bool,
    /// Indicates whether the tag is in an experimental stage.
    pub experimental: bool,
    /// Indicates whether a footer is present.
    pub footer: bool,
    /// Indicates whether or not compression is used. This flag is only used in ID3v2.2.
    pub compression: bool // v2.2 only
}

// TagFlags {{{
impl TagFlags {
    /// Creates a new `TagFlags` with all flags set to false.
    pub fn new() -> TagFlags {
        TagFlags { unsynchronization: false, extended_header: false, experimental: false, footer: false, compression: false }
    }

    /// Creates a vector representation of the flags suitable for writing to an ID3 tag.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = [0x0, ..1];
        
        if self.unsynchronization {
            bytes[0] |= 0x80;
        }

        if self.extended_header {
            bytes[0] |= 0x40;
        }

        if self.experimental {
            bytes[0] |= 0x20
        }

        if self.footer {
            bytes[0] |= 0x10;
        }

        bytes.to_vec()
    }
}
// }}}

// Tag {{{
impl ID3Tag {
    /// Creates a new ID3v2.4 tag with no frames. 
    pub fn new() -> ID3Tag {
        ID3Tag { path: None, version: [0x4, 0x0], size: 0, offset: 0, modified_offset: 0, flags: TagFlags::new(), frames: Vec::new(), rewrite: false }
    }

    /// Creates a new ID3 tag with the specified version.
    ///
    /// Only versions 3 and 4 are supported. Passing any other version will cause a panic.
    pub fn with_version(version: u8) -> ID3Tag {
        let mut tag = ID3Tag::new();
        tag.version = [version, 0];
        tag
    }

    /// Returns the default unicode encoding that should be used for this tag.
    ///
    /// For ID3 versions greater than v2.4 this returns UTF8. For versions less than v2.4 this
    /// returns UTF16.
    ///
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    /// use id3::encoding::{UTF16, UTF8};
    ///
    /// let mut tag_v3 = ID3Tag::with_version(3);
    /// assert_eq!(tag_v3.default_encoding(), UTF16);
    ///
    /// let mut tag_v4 = ID3Tag::with_version(4);
    /// assert_eq!(tag_v4.default_encoding(), UTF8);
    /// ```
    #[inline]
    pub fn default_encoding(&self) -> encoding::Encoding {
        if self.version[0] >= 4 {
            encoding::UTF8
        } else {
            encoding::UTF16
        }
    }

    /// Returns a vector of references to all frames in the tag.
    ///
    /// # Example
    /// ```
    /// use id3::{ID3Tag, Frame};
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// tag.add_frame(Frame::new("TPE1"));
    /// tag.add_frame(Frame::new("APIC"));
    ///
    /// assert_eq!(tag.get_frames().len(), 2);
    /// ```
    pub fn get_frames<'a>(&'a self) -> &'a Vec<Frame> {
        &self.frames
    }

    /// Returns a reference to the first frame with the specified identifier.
    ///
    /// # Example
    /// ```
    /// use id3::{ID3Tag, Frame};
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// tag.add_frame(Frame::new("TIT2"));
    ///
    /// assert!(tag.get_frame_by_id("TIT2").is_some());
    /// assert!(tag.get_frame_by_id("TCON").is_none());
    /// ```
    pub fn get_frame_by_id<'a>(&'a self, id: &str) -> Option<&'a Frame> {
        for frame in self.frames.iter() {
            if frame.id.as_slice() == id {
                return Some(frame);
            }
        }

        None
    }

    /// Returns a vector of references to frames with the specified identifier.
    ///
    /// # Example
    /// ```
    /// use id3::{ID3Tag, Frame};
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// tag.add_frame(Frame::new("TXXX"));
    /// tag.add_frame(Frame::new("TXXX"));
    /// tag.add_frame(Frame::new("TALB"));
    ///
    /// assert_eq!(tag.get_frames_by_id("TXXX").len(), 2);
    /// assert_eq!(tag.get_frames_by_id("TALB").len(), 1);
    /// ```
    pub fn get_frames_by_id<'a>(&'a self, id: &str) -> Vec<&'a Frame> {
        let mut matches = Vec::new();
        for frame in self.frames.iter() {
            if frame.id.as_slice() == id {
                matches.push(frame);
            }
        }

        matches
    }

    /// Adds a frame to the tag.
    ///
    /// # Example
    /// ```
    /// use id3::{ID3Tag, Frame};
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.add_frame(Frame::new("TALB"));
    /// assert_eq!(tag.get_frames()[0].id.as_slice(), "TALB");
    /// ```
    pub fn add_frame(&mut self, mut frame: Frame) {
        frame.generate_uuid();
        frame.offset = 0;
        self.frames.push(frame);
    }

    /// Adds a text frame using the default text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.add_text_frame("TCON", "Metal");
    /// assert_eq!(tag.get_frame_by_id("TCON").unwrap().contents.text().as_slice(), "Metal");
    /// ```
    pub fn add_text_frame(&mut self, id: &str, text: &str) {
        let encoding = self.default_encoding();
        self.add_text_frame_enc(id, text, encoding);
    }

    /// Adds a text frame using the specified text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.add_text_frame_enc("TRCK", "1/13", UTF16);
    /// assert_eq!(tag.get_frame_by_id("TRCK").unwrap().contents.text().as_slice(), "1/13");
    /// ```
    pub fn add_text_frame_enc(&mut self, id: &str, text: &str, encoding: encoding::Encoding) {
        self.remove_frames_by_id(id);
       
        let mut frame = Frame::new(id);
        frame.encoding = encoding;
        frame.contents = TextContent(String::from_str(text));

        self.add_frame(frame);
    }

    /// Removes the frame with the specified uuid.
    /// 
    /// # Example
    /// ```
    /// use id3::{ID3Tag, Frame};
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// tag.add_frame(Frame::new("TPE2"));
    /// assert_eq!(tag.get_frames().len(), 1);
    ///
    /// let uuid = tag.get_frames()[0].uuid.clone();
    /// tag.remove_frame_by_uuid(uuid.as_slice());
    /// assert_eq!(tag.get_frames().len(), 0);
    /// ```
    pub fn remove_frame_by_uuid(&mut self, uuid: &[u8]) {
        let mut i = 0;
        for f in self.frames.iter() {
            if f.uuid.as_slice() == uuid {
                break;
            }
            i += 1;
        }

        if i < self.frames.len() {
            if self.frames[i].offset != 0 && self.frames[i].offset < self.modified_offset {
                self.modified_offset = self.frames[i].offset;
            }
            self.frames.remove(i);
        }
    }

    /// Removes all frames with the specified identifier.
    ///
    /// # Example
    /// ```
    /// use id3::{ID3Tag, Frame};
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// tag.add_frame(Frame::new("TXXX"));
    /// tag.add_frame(Frame::new("TXXX"));
    /// tag.add_frame(Frame::new("USLT"));
    ///
    /// assert_eq!(tag.get_frames().len(), 3);
    ///
    /// tag.remove_frames_by_id("TXXX");
    /// assert_eq!(tag.get_frames().len(), 1);
    ///
    /// tag.remove_frames_by_id("USLT");
    /// assert_eq!(tag.get_frames().len(), 0);
    /// ```   
    pub fn remove_frames_by_id(&mut self, id: &str) {
        let mut modified_offset: u64 = 0;
        let set_modified_offset = |m: &mut u64, o: u64| {
            if (*m == 0 || o < *m) && o != 0 {
                *m = o;
            }
            false
        };

        self.frames.retain(|f: &Frame| f.id.as_slice() != id || set_modified_offset(&mut modified_offset, f.offset));

        if modified_offset != 0 && modified_offset < self.modified_offset {
            self.modified_offset = modified_offset;
        }
    }

    /// Returns the `TextContent` string for the frame with the specified identifier.
    /// Returns `None` if the frame with the specified ID can't be found or if the contents is not
    /// `TextContent`.
    fn text_for_frame_id(&self, id: &str) -> Option<String> {
        match self.get_frame_by_id(id) {
            Some(frame) => match frame.contents {
                TextContent(ref text) => Some(text.clone()),
                _ => None
            },
            None => None
        }
    }

    // Getters/Setters {{{
    /// Returns a vector of the user defined text frames' (TXXX) key/value pairs.
    ///
    /// # Example
    /// ```
    /// use id3::{ID3Tag, Frame, ExtendedTextContent};
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// let mut frame = Frame::new("TXXX");
    /// frame.contents = ExtendedTextContent((String::from_str("key1"),
    ///     String::from_str("value1")));
    /// tag.add_frame(frame);
    ///
    /// let mut frame = Frame::new("TXXX");
    /// frame.contents = ExtendedTextContent((String::from_str("key2"), 
    ///     String::from_str("value2")));
    /// tag.add_frame(frame);
    ///
    /// assert_eq!(tag.txxx().len(), 2);
    /// assert!(tag.txxx().contains(&(String::from_str("key1"), String::from_str("value1"))));
    /// assert!(tag.txxx().contains(&(String::from_str("key2"), String::from_str("value2"))));
    /// ```
    pub fn txxx(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for frame in self.get_frames_by_id("TXXX").iter() {
            match frame.contents {
                ExtendedTextContent((ref key, ref value)) => out.push((key.clone(), value.clone())),
                _ => { }
            }
        }

        out
    }

    /// Adds a user defined text frame (TXXX).
    ///
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// tag.add_txxx("key1", "value1");
    /// tag.add_txxx("key2", "value2");
    ///
    /// assert_eq!(tag.txxx().len(), 2);
    /// assert!(tag.txxx().contains(&(String::from_str("key1"), String::from_str("value1"))));
    /// assert!(tag.txxx().contains(&(String::from_str("key2"), String::from_str("value2"))));
    /// ```
    pub fn add_txxx(&mut self, key: &str, value: &str) {
        let encoding = self.default_encoding();
        self.add_txxx_enc(key, value, encoding);
    }

    /// Adds a user defined text frame (TXXX) using the specified text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// tag.add_txxx_enc("key1", "value1", UTF16);
    /// tag.add_txxx_enc("key2", "value2", UTF16);
    ///
    /// assert_eq!(tag.txxx().len(), 2);
    /// assert!(tag.txxx().contains(&(String::from_str("key1"), String::from_str("value1"))));
    /// assert!(tag.txxx().contains(&(String::from_str("key2"), String::from_str("value2"))));
    /// ```
    pub fn add_txxx_enc(&mut self, key: &str, value: &str, encoding: encoding::Encoding) {
        self.remove_txxx(Some(key), None);

        let mut frame = Frame::new("TXXX");
        frame.encoding = encoding;
        frame.contents = ExtendedTextContent((String::from_str(key), String::from_str(value)));
        
        self.add_frame(frame);
    }

    /// Removes the user defined text frame (TXXX) with the specified key and value.
    /// A key or value may be `None` to specify a wildcard value.
    /// 
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// tag.add_txxx("key1", "value1");
    /// tag.add_txxx("key2", "value2");
    /// tag.add_txxx("key3", "value2");
    /// tag.add_txxx("key4", "value3");
    /// tag.add_txxx("key5", "value4");
    /// assert_eq!(tag.txxx().len(), 5);
    ///
    /// tag.remove_txxx(Some("key1"), None);
    /// assert_eq!(tag.txxx().len(), 4);
    ///
    /// tag.remove_txxx(None, Some("value2"));
    /// assert_eq!(tag.txxx().len(), 2);
    ///
    /// tag.remove_txxx(Some("key4"), Some("value3"));
    /// assert_eq!(tag.txxx().len(), 1);
    ///
    /// tag.remove_txxx(None, None);
    /// assert_eq!(tag.txxx().len(), 0);
    /// ```
    pub fn remove_txxx(&mut self, key: Option<&str>, value: Option<&str>) {
        let mut modified_offset: u64 = 0;
        let set_modified_offset = |m: &mut u64, o: u64| {
            if (*m == 0 || o < *m) && o != 0 {
                *m = o;
            }
        };

        self.frames.retain(|f: &Frame| {
            let mut key_match = false;
            let mut value_match = false;

            if f.id.as_slice() == "TXXX" {
                match f.contents {
                    ExtendedTextContent((ref k, ref v)) => {
                        match key {
                            Some(s) => key_match = s == k.as_slice(),
                            None => key_match = true
                        }

                        match value {
                            Some(s) => value_match = s == v.as_slice(),
                            None => value_match = true 
                        }
                    },
                    _ => { // remove frames that we can't parse
                        key_match = true;
                        value_match = true;
                    }
                }
            }

            if key_match && value_match {
                set_modified_offset(&mut modified_offset, f.offset);
            }

            !(key_match && value_match) // true if we want to keep the item
        });

        if modified_offset != 0 && modified_offset < self.modified_offset {
            self.modified_offset = modified_offset;
        }
    }

    /// Returns a vector of references to the pictures in the tag.
    ///
    /// # Example
    /// ```
    /// use id3::{ID3Tag, Frame, Picture, PictureContent};
    ///
    /// let mut tag = ID3Tag::new();
    /// 
    /// let mut frame = Frame::new("APIC");
    /// frame.contents = PictureContent(Picture::new());
    /// tag.add_frame(frame);
    ///
    /// let mut frame = Frame::new("APIC");
    /// frame.contents = PictureContent(Picture::new());
    /// tag.add_frame(frame);
    ///
    /// assert_eq!(tag.pictures().len(), 2);
    /// ```
    pub fn pictures(&self) -> Vec<&Picture> {
        let mut pictures = Vec::new();
        for frame in self.get_frames_by_id("APIC").iter() {
            match frame.contents {
                PictureContent(ref picture) => pictures.push(picture),
                _ => { }
            }
        }
        pictures
    }

    /// Adds a picture frame (APIC).
    /// Any other pictures with the same type will be removed from the tag.
    ///
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    /// use id3::picture_type::Other;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.add_picture("image/jpeg", Other, [0]);
    /// tag.add_picture("image/png", Other, [0]);
    /// assert_eq!(tag.pictures().len(), 1);
    /// assert_eq!(tag.pictures()[0].mime_type.as_slice(), "image/png");
    /// ```
    pub fn add_picture(&mut self, mime_type: &str, picture_type: picture_type::PictureType, data: &[u8]) {
        self.add_picture_enc(mime_type, picture_type, "", data, encoding::Latin1);
    }

    /// Adds a picture frame (APIC) using the specified text encoding.
    /// Any other pictures with the same type will be removed from the tag.
    ///
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    /// use id3::picture_type::Other;
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.add_picture_enc("image/jpeg", Other, "", [0], UTF16);
    /// tag.add_picture_enc("image/png", Other, "", [0], UTF16);
    /// assert_eq!(tag.pictures().len(), 1);
    /// assert_eq!(tag.pictures()[0].mime_type.as_slice(), "image/png");
    /// ```
    pub fn add_picture_enc(&mut self, mime_type: &str, picture_type: picture_type::PictureType, description: &str, data: &[u8], encoding: encoding::Encoding)
    {
        self.remove_picture_type(picture_type);

        let mut frame = Frame::new("APIC");
        frame.encoding = encoding;
        frame.contents = PictureContent(Picture { mime_type: String::from_str(mime_type), picture_type: picture_type, description: String::from_str(description), data: data.to_vec() } );

        self.add_frame(frame);
    }

    /// Removes all pictures of the specified type.
    ///
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    /// use id3::picture_type::{CoverFront, Other};
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.add_picture("image/jpeg", CoverFront, [0]);
    /// tag.add_picture("image/png", Other, [0]);
    /// assert_eq!(tag.pictures().len(), 2);
    ///
    /// tag.remove_picture_type(CoverFront);
    /// assert_eq!(tag.pictures().len(), 1);
    /// assert_eq!(tag.pictures()[0].picture_type, Other);
    /// ```
    pub fn remove_picture_type(&mut self, picture_type: picture_type::PictureType) {
        let mut modified_offset: u64 = 0;
        let set_modified_offset = |m: &mut u64, o: u64| {
            if (*m == 0 || o < *m) && o != 0 {
                *m = o;
            }
            false
        };

        self.frames.retain(|f: &Frame| {
            if f.id.as_slice() == "APIC" {
                let pic = match f.contents {
                    PictureContent(ref picture) => picture,
                    _ => return false
                };

                if pic.picture_type == picture_type {
                   set_modified_offset(&mut modified_offset, f.offset);
                }

                return pic.picture_type != picture_type
            }

            true
        });

        if modified_offset != 0 && modified_offset < self.modified_offset {
            self.modified_offset = modified_offset;
        }
    }

    /// Returns a vector of the user comment frames' (COMM) key/value pairs.
    ///
    /// # Example
    /// ```
    /// use id3::{ID3Tag, Frame, CommentContent};
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// let mut frame = Frame::new("COMM");
    /// frame.contents = CommentContent((String::from_str("key1"), String::from_str("value1")));
    /// tag.add_frame(frame);
    ///
    /// let mut frame = Frame::new("COMM");
    /// frame.contents = CommentContent((String::from_str("key2"), String::from_str("value2")));
    /// tag.add_frame(frame);
    ///
    /// assert_eq!(tag.comments().len(), 2);
    /// assert!(tag.comments().contains(&(String::from_str("key1"), String::from_str("value1"))));
    /// assert!(tag.comments().contains(&(String::from_str("key2"), String::from_str("value2"))));
    /// ```
    pub fn comments(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for frame in self.get_frames_by_id("COMM").iter() {
            match frame.contents {
                CommentContent(ref text) => out.push(text.clone()),
                _ => { }
            }
        }

        out
    }
 
    /// Adds a user comment frame (COMM).
    ///
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// tag.add_comment("key1", "value1");
    /// tag.add_comment("key2", "value2");
    ///
    /// assert_eq!(tag.comments().len(), 2);
    /// assert!(tag.comments().contains(&(String::from_str("key1"), String::from_str("value1"))));
    /// assert!(tag.comments().contains(&(String::from_str("key2"), String::from_str("value2"))));
    /// ```
    pub fn add_comment(&mut self, description: &str, text: &str) {
        let encoding = self.default_encoding();
        self.add_comment_enc(description, text, encoding);
    }

    /// Adds a user comment frame (COMM) using the specified text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// tag.add_comment_enc("key1", "value1", UTF16);
    /// tag.add_comment_enc("key2", "value2", UTF16);
    ///
    /// assert_eq!(tag.comments().len(), 2);
    /// assert!(tag.comments().contains(&(String::from_str("key1"), String::from_str("value1"))));
    /// assert!(tag.comments().contains(&(String::from_str("key2"), String::from_str("value2"))));
    /// ```
    pub fn add_comment_enc(&mut self, description: &str, text: &str, encoding: encoding::Encoding) {
        self.remove_comment(Some(description), None);

        let mut frame = Frame::new("COMM");
        frame.encoding = encoding;
        frame.contents = CommentContent((String::from_str(description), String::from_str(text)));
       
        self.add_frame(frame);
    }

    /// Removes the user comment frame (COMM) with the specified key and value.
    /// A key or value may be `None` to specify a wildcard value.
    /// 
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    ///
    /// let mut tag = ID3Tag::new();
    ///
    /// tag.add_comment("key1", "value1");
    /// tag.add_comment("key2", "value2");
    /// tag.add_comment("key3", "value2");
    /// tag.add_comment("key4", "value3");
    /// tag.add_comment("key5", "value4");
    /// assert_eq!(tag.comments().len(), 5);
    ///
    /// tag.remove_comment(Some("key1"), None);
    /// assert_eq!(tag.comments().len(), 4);
    ///
    /// tag.remove_comment(None, Some("value2"));
    /// assert_eq!(tag.comments().len(), 2);
    ///
    /// tag.remove_comment(Some("key4"), Some("value3"));
    /// assert_eq!(tag.comments().len(), 1);
    ///
    /// tag.remove_comment(None, None);
    /// assert_eq!(tag.comments().len(), 0);
    /// ```
    pub fn remove_comment(&mut self, key: Option<&str>, value: Option<&str>) {
        let mut modified_offset: u64 = 0;
        let set_modified_offset = |m: &mut u64, o: u64| {
            if (*m == 0 || o < *m) && o != 0 {
                *m = o;
            }
        };

        self.frames.retain(|f: &Frame| {
            let mut key_match = false;
            let mut value_match = false;

            if f.id.as_slice() == "COMM" {
                match f.contents {
                    CommentContent((ref k, ref v)) =>  {
                        match key {
                            Some(s) => key_match = s == k.as_slice(),
                            None => key_match = true
                        }

                        match value {
                            Some(s) => value_match = s == v.as_slice(),
                            None => value_match = true 
                        }
                    },
                    _ => { // remove frames that we can't parse
                        key_match = true;
                        value_match = true;
                    }
                }
            }

            if key_match && value_match {
                set_modified_offset(&mut modified_offset, f.offset);
            }

            !(key_match && value_match) // true if we want to keep the item
        });

        if modified_offset != 0 && modified_offset < self.modified_offset {
            self.modified_offset = modified_offset;
        }
    }

    /// Sets the artist (TPE1) using the specified text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::{AudioTag, ID3Tag};
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.set_artist_enc("artist", UTF16);
    /// assert_eq!(tag.artist().unwrap().as_slice(), "artist");
    /// ```
    pub fn set_artist_enc(&mut self, artist: &str, encoding: encoding::Encoding) {
        self.add_text_frame_enc("TPE1", artist, encoding);
    }

    /// Sets the album artist (TPE2) using the specified text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::{AudioTag, ID3Tag};
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.set_album_artist_enc("album artist", UTF16);
    /// assert_eq!(tag.album_artist().unwrap().as_slice(), "album artist");
    /// ```
    pub fn set_album_artist_enc(&mut self, album_artist: &str, encoding: encoding::Encoding) {
        self.remove_frames_by_id("TSOP");
        self.add_text_frame_enc("TPE2", album_artist, encoding);
    }

    /// Sets the album (TALB) using the specified text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::{AudioTag, ID3Tag};
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.set_album_enc("album", UTF16);
    /// assert_eq!(tag.album().unwrap().as_slice(), "album");
    /// ```
    pub fn set_album_enc(&mut self, album: &str, encoding: encoding::Encoding) {
        self.add_text_frame_enc("TALB", album, encoding);
    }

    /// Sets the song title (TIT2) using the specified text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::{AudioTag, ID3Tag};
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.set_title_enc("title", UTF16);
    /// assert_eq!(tag.title().unwrap().as_slice(), "title");
    /// ```
    pub fn set_title_enc(&mut self, title: &str, encoding: encoding::Encoding) {
        self.remove_frames_by_id("TSOT");
        self.add_text_frame_enc("TIT2", title, encoding);
    }

    /// Sets the genre (TCON) using the specified text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::{AudioTag, ID3Tag};
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.set_genre_enc("genre", UTF16);
    /// assert_eq!(tag.genre().unwrap().as_slice(), "genre");
    /// ```
    pub fn set_genre_enc(&mut self, genre: &str, encoding: encoding::Encoding) {
        self.add_text_frame_enc("TCON", genre, encoding);
    }

    /// Returns the year (TYER).
    /// Returns `None` if the year frame could not be found or if it could not be parsed.
    ///
    /// # Example
    /// ```
    /// use id3::{ID3Tag, Frame, TextContent};
    ///
    /// let mut tag = ID3Tag::new();
    /// assert!(tag.year().is_none());
    ///
    /// let mut frame_valid = Frame::new("TYER");
    /// frame_valid.contents = TextContent(String::from_str("2014"));
    /// tag.add_frame(frame_valid);
    /// assert_eq!(tag.year().unwrap(), 2014);
    ///
    /// tag.remove_frames_by_id("TYER");
    ///
    /// let mut frame_invalid = Frame::new("TYER");
    /// frame_invalid.contents = TextContent(String::from_str("nope"));
    /// tag.add_frame(frame_invalid);
    /// assert!(tag.year().is_none());
    /// ```
    pub fn year(&self) -> Option<uint> {
        match self.get_frame_by_id("TYER") {
            Some(frame) => {
                match frame.contents {
                    TextContent(ref text) => from_str(text.as_slice()),
                    _ => None
                }
            },
            None => None
        }
    }

    /// Sets the year (TYER).
    ///
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.set_year(2014);
    /// assert_eq!(tag.year().unwrap(), 2014);
    /// ```
    pub fn set_year(&mut self, year: uint) {
        self.add_text_frame_enc("TYER", format!("{}", year).as_slice(), encoding::Latin1);
    }

    /// Sets the year (TYER) using the specified text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::ID3Tag;
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.set_year_enc(2014, UTF16);
    /// assert_eq!(tag.year().unwrap(), 2014);
    /// ```
    pub fn set_year_enc(&mut self, year: uint, encoding: encoding::Encoding) {
        self.add_text_frame_enc("TYER", format!("{}", year).as_slice(), encoding);
    }

    /// Returns the (track, total_tracks) tuple.
    fn track_pair(&self) -> Option<(u32, Option<u32>)> {
        match self.get_frame_by_id("TRCK") {
            Some(frame) => {
                match frame.contents {
                    TextContent(ref text) => {
                        let split: Vec<&str> = text.as_slice().splitn(2, '/').collect();

                        let total_tracks = if split.len() == 2 {
                            match from_str(split[1]) {
                                Some(total_tracks) => Some(total_tracks),
                                None => return None
                            }
                        } else {
                            None
                        };

                        match from_str(split[0]) {
                            Some(track) => Some((track, total_tracks)),
                            None => None
                        }
                    },
                    _ => None
                }
            },
            None => None
        }
    }

    /// Sets the track number (TRCK) using the specified text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::{AudioTag, ID3Tag};
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.set_track_enc(5, UTF16);
    /// assert_eq!(tag.track().unwrap(), 5);
    /// ```
    pub fn set_track_enc(&mut self, track: u32, encoding: encoding::Encoding) {
        let text = match self.track_pair().and_then(|(_, total_tracks)| total_tracks) {
            Some(n) => format!("{}/{}", track, n),
            None => format!("{}", track)
        };

        self.add_text_frame_enc("TRCK", text.as_slice(), encoding);
    }


    /// Sets the total number of tracks (TRCK) using the specified text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::{AudioTag, ID3Tag};
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.set_total_tracks_enc(12, UTF16);
    /// assert_eq!(tag.total_tracks().unwrap(), 12);
    /// ```
    pub fn set_total_tracks_enc(&mut self, total_tracks: u32, encoding: encoding::Encoding) {
        let text = match self.track_pair() {
            Some((track, _)) => format!("{}/{}", track, total_tracks),
            None => format!("1/{}", total_tracks)
        };

        self.add_text_frame_enc("TRCK", text.as_slice(), encoding);
    }


    /// Sets the lyrics text (USLT) using the specified text encoding.
    ///
    /// # Example
    /// ```
    /// use id3::{AudioTag, ID3Tag};
    /// use id3::encoding::UTF16;
    ///
    /// let mut tag = ID3Tag::new();
    /// tag.set_lyrics_enc("lyrics", UTF16);
    /// assert_eq!(tag.lyrics().unwrap().as_slice(), "lyrics");
    /// ```
    pub fn set_lyrics_enc(&mut self, text: &str, encoding: encoding::Encoding) {
        self.remove_frames_by_id("USLT");

        let mut frame = Frame::new("USLT");
        frame.encoding = encoding;
        frame.contents = LyricsContent(String::from_str(text));
        
        self.add_frame(frame);
    }
    //}}}
}
impl AudioTag for ID3Tag {
    // Reading/Writing {{{
    fn load(path: &Path) -> TagResult<ID3Tag> {
        let mut tag = ID3Tag::new();
        tag.path = Some(path.clone());

        let mut file = try!(File::open(path));

        let identifier = try!(file.read_exact(3));
        if identifier.as_slice() != "ID3".as_bytes() {
            debug!("no id3 tag found");
            return Err(TagError::new(InvalidInputError, "file does not contain an id3 tag"))
        }

        try!(file.read(tag.version));

        debug!("tag version {}", tag.version[0]);

        if tag.version[0] != 0x2 && tag.version[0] != 0x3 && tag.version[0] != 0x4 {
            return Err(TagError::new(InvalidInputError, "unsupported id3 tag version"));
        }

        if tag.version[0] == 0x2 {
            tag.rewrite = true;
        }

        let flags = try!(file.read_byte());
        tag.flags.unsynchronization = flags & 0x80 != 0;
        if tag.version[0] == 0x2 {
            tag.flags.compression = flags & 0x40 != 0;
        } else {
            tag.flags.extended_header = flags & 0x40 != 0;
            tag.flags.experimental = flags & 0x20 != 0;
            tag.flags.footer = flags & 0x10 != 0; // TODO read the footer?
        }

        if tag.flags.unsynchronization {
            debug!("unsynchronization is unsupported");
            return Err(TagError::new(UnsupportedFeatureError, "unsynchronization is not supported"))
        } else if tag.flags.compression {
            debug!("id3v2.2 compression is unsupported");
            return Err(TagError::new(UnsupportedFeatureError, "id3v2.2 compression is not supported"));
        }

        tag.size = util::unsynchsafe(try!(file.read_be_u32()));

        // TODO actually use the extended header data
        if tag.flags.extended_header {
            let ext_size = util::unsynchsafe(try!(file.read_be_u32()));
            try!(file.seek(ext_size as i64, SeekCur));
        }

        while try!(file.tell()) < tag.size as u64 + 10 {
            let frame = match Frame::read(tag.version[0], &mut file) {
                Ok(opt) => match opt {
                    Some(frame) => frame,
                    None => break //padding
                },
                Err(err) => {
                    match err.kind {
                        UnsupportedFeatureError => continue,
                        _ => {
                            debug!("{}", err);
                            return Err(err);
                        }
                    }
                }
            };

            tag.frames.push(frame);
        }

        if tag.version[0] == 0x2 {
            tag.version = [0x4, 0x0];
        }

        tag.offset = try!(file.tell());
        tag.modified_offset = tag.offset;

        return Ok(tag);
    }

    fn save(&mut self) -> TagResult<()> {
        let path = self.path.clone().unwrap();
        self.write(&path)
    }

    fn skip_metadata(path: &Path) -> Vec<u8> {
        macro_rules! try_io {
            ($file:ident, $action:expr) => {
                match $action { 
                    Ok(bytes) => bytes, 
                    Err(_) => {
                        match $file.seek(0, SeekSet) {
                            Ok(_) => {
                                match $file.read_to_end() {
                                    Ok(bytes) => return bytes,
                                    Err(_) => return Vec::new()
                                }
                            },
                            Err(_) => return Vec::new()
                        }
                    }
                }
            }
        }

        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(_) => return Vec::new()
        };

        let ident = try_io!(file, file.read_exact(3));
        if ident.as_slice() == b"ID3" {
            try_io!(file, file.seek(3, SeekCur));
            let offset = 10 + util::unsynchsafe(try_io!(file, file.read_be_u32()));   
            try_io!(file, file.seek(offset as i64, SeekSet));
        } else {
            try_io!(file, file.seek(0, SeekSet));
        }

        try_io!(file, file.read_to_end())
    }

    fn is_candidate(path: &Path, _: Option<ID3Tag>) -> bool {
        macro_rules! try_or_false {
            ($action:expr) => {
                match $action { 
                    Ok(result) => result, 
                    Err(_) => return false 
                }
            }
        }

        (try_or_false!((try_or_false!(File::open(path))).read_exact(3))).as_slice() == b"ID3"
    }

    fn write(&mut self, path: &Path) -> TagResult<()> {
        static DEFAULT_FILE_DISCARD: [&'static str, ..11] = ["AENC", "ETCO", "EQUA", "MLLT", "POSS", "SYLT", "SYTC", "RVAD", "TENC", "TLEN", "TSIZ"];

        let file_changed = self.path.is_none() || self.path.clone().unwrap() != *path;

        let mut rewrite = false;
        if self.rewrite || file_changed || self.flags.extended_header {
            self.flags.extended_header = false; // don't support writing extended header
            rewrite = true;
            self.version = [0x4, 0x0];
        }

        debug!("perform a rewrite? {}", rewrite);

        self.path = Some(path.clone());

        let mut data_cache: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();

        let mut new_size = 0;
        for frame in self.frames.iter_mut() {
            let data = frame.to_bytes(self.version[0]);
            new_size += data.len() as u32; 
            data_cache.insert(frame.uuid.clone(), data);
        }

        if new_size > self.size {
            rewrite = true;
        }

        let padding_bytes = 2048;
        new_size += padding_bytes;

        if rewrite {
            self.size = new_size;

            let data = AudioTag::skip_metadata(path);

            let mut file = try!(File::open_mode(path, std::io::Truncate, std::io::Write));

            try!(file.write(b"ID3"));
            try!(file.write(self.version)); 
            try!(file.write(self.flags.to_bytes().as_slice()));
            try!(file.write_be_u32(util::synchsafe(self.size)));

            let mut remove_uuid = Vec::new();
            for frame in self.frames.iter_mut() {
                // discard the frame if it is not new, and the flags/id say it should be discarded
                if frame.offset != 0 && (frame.flags.tag_alter_preservation || (file_changed && (frame.flags.file_alter_preservation || DEFAULT_FILE_DISCARD.contains(&frame.id.as_slice())))) {
                    debug!("dicarding {} since tag/file changed", frame.id);
                    remove_uuid.push(frame.uuid.clone());
                } else {
                    frame.offset = try!(file.tell());
                    debug!("writing {}", frame.id);
                    match data_cache.get(&frame.uuid) {
                        Some(data) => try!(file.write(data.as_slice())),
                        None => try!(file.write(frame.to_bytes(self.version[0]).as_slice()))
                    }
                }
            }

            self.frames.retain(|frame: &Frame| !remove_uuid.contains(&frame.uuid));

            self.offset = try!(file.tell());
            self.modified_offset = self.offset;

            // write padding
            for _ in range(0, padding_bytes) {
                try!(file.write_u8(0x0));
            }

            // write the remaining data
            try!(file.write(data.as_slice()));
        } else {
            let mut file = try!(File::open_mode(path, std::io::Open, std::io::Write));

            try!(file.seek(self.modified_offset as i64, SeekSet));
            let mut remove_uuid = Vec::new();
            for frame in self.frames.iter_mut() {
                // discard the frame if it is not new, and the flags say it should be discarded
                if frame.offset != 0 && frame.flags.tag_alter_preservation {
                    debug!("dicarding {} since tag changed", frame.id);
                    remove_uuid.push(frame.uuid.clone());
                } else if frame.offset == 0 || frame.offset > self.modified_offset {
                    debug!("writing {}", frame.id);
                    frame.offset = try!(file.tell());
                    try!(file.write(frame.to_bytes(self.version[0]).as_slice()));
                }
            }

            self.frames.retain(|frame: &Frame| !remove_uuid.contains(&frame.uuid));

            let old_offset = self.offset;
            self.offset = try!(file.tell());
            self.modified_offset = self.offset;

            if self.offset < old_offset {
                for _ in range(self.offset, old_offset) {
                    try!(file.write_u8(0x0));
                }
            }
        }

        Ok(())
    }
    //}}}
    
    fn artist(&self) -> Option<String> {
        self.text_for_frame_id("TPE1")
    }

    fn set_artist(&mut self, artist: &str) {
        self.add_text_frame("TPE1", artist);
    }

    fn remove_artist(&mut self) {
        self.remove_frames_by_id("TPE1");
    }

    fn album_artist(&self) -> Option<String> {
        self.text_for_frame_id("TPE2")
    }

    fn set_album_artist(&mut self, album_artist: &str) {
        self.add_text_frame("TPE2", album_artist);
    }

    fn remove_album_artist(&mut self) {
        self.remove_frames_by_id("TPE2");
    }

    fn album(&self) -> Option<String> {
        self.text_for_frame_id("TALB")
    }

    fn set_album(&mut self, album: &str) {
        self.remove_frames_by_id("TSOP");
        self.add_text_frame("TALB", album);
    }

    fn remove_album(&mut self) {
        self.remove_frames_by_id("TSOP");
        self.remove_frames_by_id("TALB");
    }

    fn title(&self) -> Option<String> {
        self.text_for_frame_id("TIT2")
    }

    fn set_title(&mut self, title: &str) {
        self.add_text_frame("TIT2", title);
    }

    fn remove_title(&mut self) {
        self.remove_frames_by_id("TIT2");
    }

    fn genre(&self) -> Option<String> {
        self.text_for_frame_id("TCON")
    }

    fn set_genre(&mut self, genre: &str) {
        self.add_text_frame("TCON", genre);
    }

    fn remove_genre(&mut self) {
        self.remove_frames_by_id("TCON");
    }

    fn track(&self) -> Option<u32> {
        self.track_pair().and_then(|(track, _)| Some(track))
    }

    fn set_track(&mut self, track: u32) {
        self.set_track_enc(track, encoding::Latin1);
    }

    fn remove_track(&mut self) {
        self.remove_frames_by_id("TRCK");
    }

    fn total_tracks(&self) -> Option<u32> {
        self.track_pair().and_then(|(_, total_tracks)| total_tracks)
    }

    fn set_total_tracks(&mut self, total_tracks: u32) {
        self.set_total_tracks_enc(total_tracks, encoding::Latin1);
    }

    fn remove_total_tracks(&mut self) {
        match self.track_pair() {
            Some((track, _)) => self.add_text_frame("TALB", format!("{}", track).as_slice()),
            None => {}
        }
    }

    fn lyrics(&self) -> Option<String> {
        match self.get_frame_by_id("USLT") {
            Some(frame) => match frame.contents {
                LyricsContent(ref text) => Some(text.clone()),
                _ => None
            },
            None => None
        }
    }

    fn set_lyrics(&mut self, text: &str) {
        let encoding = self.default_encoding();
        self.set_lyrics_enc(text, encoding);
    }

    fn remove_lyrics(&mut self) {
        self.remove_frames_by_id("USLT");
    }

    fn set_picture(&mut self, mime_type: &str, data: &[u8]) {
        self.remove_picture();
        self.add_picture(mime_type, picture_type::Other, data);
    }

    fn remove_picture(&mut self) {
        self.remove_frames_by_id("APIC");
    }

    fn all_metadata(&self) -> Vec<(String, String)> {
        let mut metadata = Vec::new();
        for frame in self.frames.iter() {
            match frame.text() {
                Some(text) => metadata.push((frame.id.clone(), text)),
                None => {}
            }
        }
        metadata
    }
}
// }}}

// Tests {{{
#[cfg(test)]
mod tests {
    use tag::TagFlags;

    #[test]
    fn test_flags_to_bytes() {
        let mut flags = TagFlags::new();
        assert_eq!(flags.to_bytes(), vec!(0x0));
        flags.unsynchronization = true;
        flags.extended_header = true;
        flags.experimental = true;
        flags.footer = true;
        assert_eq!(flags.to_bytes(), vec!(0xF0));
    }
}
// }}}
