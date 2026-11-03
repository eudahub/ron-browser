//! Images: where their bytes may come from, and decoding them. Everything is in Rust, so that no
//! C++ decoder ever reads image files from a page.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Cursor;
use std::path::{Component, Path, PathBuf};

use image::{ImageReader, Limits};

/// Larger images are refused before anything is allocated for them.
const MAX_DIMENSION: u32 = 16_384;
/// What one image may use while it is decoded.
const MAX_DECODING_BYTES: u64 = 512 * 1024 * 1024;
/// What all images of a page may hold; further ones count as broken.
const MAX_TOTAL_BYTES: usize = 1024 * 1024 * 1024;
/// Larger files are not read.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// How many different SVGs in a page get drawn, as each takes work that blocks the window.
const MAX_DRAWN_SVGS: usize = 4096;

/// Gives the bytes of what a page refers to.
pub trait ResourceLoader {
    fn load(&mut self, url: &str) -> Option<Vec<u8>>;
}

/// Loads nothing, for pages that have no place to load from.
pub struct NoResources;

impl ResourceLoader for NoResources {
    fn load(&mut self, _url: &str) -> Option<Vec<u8>> {
        None
    }
}

/// Reads files from one directory and below it, and nothing else: no other scheme, and no
/// way out by `..`, an absolute path or a symbolic link.
pub struct LocalFiles {
    root: Option<PathBuf>,
}

impl LocalFiles {
    /// `root` is the directory of the page. Without one, nothing can be loaded.
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.canonicalize().ok(),
        }
    }

    /// Finds the file for a URL, when it is allowed to be read.
    #[must_use]
    pub fn resolve(&self, url: &str) -> Option<PathBuf> {
        let root = self.root.as_ref()?;
        let url = url.trim();
        // Anything with a scheme, such as http: or file:, or that starts at a server.
        let first = url.find(['/', '?', '#']).unwrap_or(url.len());
        if url[..first].contains(':') || url.starts_with("//") {
            return None;
        }
        let path = url.split(['?', '#']).next()?;
        if path.is_empty() {
            return None;
        }
        let decoded = percent_decode(path)?;
        let relative = Path::new(&decoded);
        if relative
            .components()
            .any(|part| matches!(part, Component::RootDir | Component::Prefix(_)))
        {
            return None;
        }
        // The real path also resolves symbolic links, which may lead elsewhere.
        let file = root.join(relative).canonicalize().ok()?;
        file.starts_with(root).then_some(file)
    }
}

impl ResourceLoader for LocalFiles {
    fn load(&mut self, url: &str) -> Option<Vec<u8>> {
        let file = self.resolve(url)?;
        let metadata = std::fs::metadata(&file).ok()?;
        if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
            return None;
        }
        std::fs::read(file).ok()
    }
}

pub(crate) fn percent_decode(text: &str) -> Option<String> {
    if !text.contains('%') {
        return Some(text.to_owned());
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(hex) = text.get(index + 1..index + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(out).ok()?;
    // A path cannot hold these, and should not be tricked into ending early.
    (!decoded.contains('\0')).then_some(decoded)
}

/// An image ready for drawing: rows of red, green, blue and alpha bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decodes an image of any format that has a safe decoder: PNG, JPEG, GIF, WebP, BMP or SVG. Of an
/// animation, the first frame. Returns `None` for what cannot be decoded.
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<DecodedImage> {
    decode_bitmap(bytes).or_else(|| crate::svg::decode(bytes))
}

fn decode_bitmap(bytes: &[u8]) -> Option<DecodedImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODING_BYTES);
    reader.limits(limits);
    let image = reader.decode().ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(DecodedImage {
        width,
        height,
        rgba: image.into_raw(),
    })
}

/// The images of a page, each decoded once.
#[derive(Default)]
pub struct ImageStore {
    images: Vec<DecodedImage>,
    by_url: HashMap<Box<str>, Option<u32>>,
    total_bytes: usize,
    /// What an `<svg>` in the page has been drawn as, by the position of its node.
    of_svg: HashMap<usize, u32>,
    /// What has been drawn, by what the text and size of it hash to, as icons repeat.
    drawn_svgs: HashMap<u64, Option<u32>>,
}

impl ImageStore {
    /// Gets an image ready if it is not yet, whether or not this works.
    pub fn load(&mut self, url: &str, loader: &mut dyn ResourceLoader) {
        if self.by_url.contains_key(url) {
            return;
        }
        let image = loader.load(url).and_then(|bytes| decode(&bytes));
        let number = image.and_then(|image| self.add(image));
        self.by_url.insert(url.into(), number);
    }

    /// Adds an image under a URL without loading anything.
    pub fn insert(&mut self, url: &str, image: DecodedImage) {
        let number = self.add(image);
        self.by_url.insert(url.into(), number);
    }

    fn add(&mut self, image: DecodedImage) -> Option<u32> {
        if self.total_bytes + image.rgba.len() > MAX_TOTAL_BYTES {
            return None;
        }
        self.total_bytes += image.rgba.len();
        self.images.push(image);
        u32::try_from(self.images.len() - 1).ok()
    }

    /// Draws an `<svg>` of a page if it has not been yet, and notes it for its node.
    pub fn draw_svg(&mut self, node: usize, text: &str, width: f32, height: f32) {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        width.to_bits().hash(&mut hasher);
        height.to_bits().hash(&mut hasher);
        let key = hasher.finish();
        let number = match self.drawn_svgs.get(&key) {
            Some(&number) => number,
            None => {
                let number = if self.drawn_svgs.len() < MAX_DRAWN_SVGS {
                    crate::svg::render(text, width, height).and_then(|image| self.add(image))
                } else {
                    None
                };
                self.drawn_svgs.insert(key, number);
                number
            }
        };
        match number {
            Some(number) => {
                self.of_svg.insert(node, number);
            }
            None => {
                self.of_svg.remove(&node);
            }
        }
    }

    /// The number of what the `<svg>` at this node has been drawn as.
    #[must_use]
    pub fn number_of_svg(&self, node: usize) -> Option<u32> {
        self.of_svg.get(&node).copied()
    }

    /// The number of the image at a URL, unless it is missing or broken.
    #[must_use]
    pub fn number(&self, url: &str) -> Option<u32> {
        self.by_url.get(url).copied().flatten()
    }

    #[must_use]
    pub fn get(&self, number: u32) -> Option<&DecodedImage> {
        self.images.get(number as usize)
    }
}
