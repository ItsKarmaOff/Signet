//! Shared decoders for `data:` URLs (every screen goes through here).
//!
//! `data:` URLs: the app hands icons, favicons and QR codes to the screens as data URLs, and the install card hands a
//! PNG icon back the same way. GPUI cannot load them directly, so they are decoded here.

use crate::widgets::FaviconSource;
use gpui::{Image, ImageFormat};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Standard or URL-safe base64, padding optional, whitespace ignored. `None` on any other character.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in text.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Percent-decoding of a data URL payload (`%3C` and the like).
fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = ((bytes[i + 1] as char).to_digit(16), (bytes[i + 2] as char).to_digit(16))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Splits `data:<mime>[;base64],<payload>` into its MIME type and decoded bytes.
pub fn decode_data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (head, payload) = rest.split_once(',')?;
    let mut parts = head.split(';');
    let mime = parts.next().unwrap_or("").to_ascii_lowercase();
    let is_b64 = parts.any(|p| p.eq_ignore_ascii_case("base64"));
    let bytes = if is_b64 { base64_decode(payload)? } else { percent_decode(payload) };
    Some((if mime.is_empty() { "text/plain".into() } else { mime }, bytes))
}

fn format_of(mime: &str) -> Option<ImageFormat> {
    Some(match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" | "image/jpg" => ImageFormat::Jpeg,
        "image/webp" => ImageFormat::Webp,
        "image/gif" => ImageFormat::Gif,
        "image/svg+xml" => ImageFormat::Svg,
        "image/bmp" => ImageFormat::Bmp,
        "image/tiff" => ImageFormat::Tiff,
        "image/x-icon" | "image/vnd.microsoft.icon" => ImageFormat::Ico,
        _ => return None,
    })
}

thread_local! {
    static IMAGES: RefCell<HashMap<String, Option<Arc<Image>>>> = RefCell::new(HashMap::new());
}

/// A GPUI image from an image data URL (cached: favicons are redrawn on every frame).
pub fn image_from_data_url(url: &str) -> Option<Arc<Image>> {
    IMAGES.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(hit) = cache.get(url) {
            return hit.clone();
        }
        if cache.len() > 512 {
            cache.clear();
        }
        let image = decode_data_url(url).and_then(|(mime, bytes)| format_of(&mime).map(|f| Arc::new(Image::from_bytes(f, bytes))));
        cache.insert(url.to_string(), image.clone());
        image
    })
}

/// An uncached GPUI image from a data URL; `None` if the MIME type is not an image format.
pub fn decode_image(url: &str) -> Option<Arc<Image>> {
    let (mime, bytes) = decode_data_url(url)?;
    format_of(&mime).map(|f| Arc::new(Image::from_bytes(f, bytes)))
}

thread_local! {
    static SOURCES: RefCell<HashMap<String, Option<FaviconSource>>> = RefCell::new(HashMap::new());
}

/// Image source of a favicon string: `data:` URL decoded once (cached), web URL as is, anything else `None`.
pub fn favicon_source(src: Option<&str>) -> Option<FaviconSource> {
    let src = src.filter(|s| !s.is_empty())?;
    SOURCES.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(hit) = c.get(src) {
            return hit.clone();
        }
        if c.len() > 1024 {
            c.clear();
        }
        let source = if src.starts_with("data:") {
            image_from_data_url(src).map(FaviconSource::Bytes)
        } else if src.starts_with("http://") || src.starts_with("https://") {
            Some(FaviconSource::Url(src.to_string().into()))
        } else {
            None
        };
        c.insert(src.to_string(), source.clone());
        source
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for sample in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0, 255, 128, 7]] {
            assert_eq!(base64_decode(&base64_encode(sample)).as_deref(), Some(sample));
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert!(base64_decode("a*b").is_none());
    }

    #[test]
    fn data_urls_decode() {
        let (mime, bytes) = decode_data_url("data:image/png;base64,Zm9v").unwrap();
        assert_eq!((mime.as_str(), bytes.as_slice()), ("image/png", &b"foo"[..]));
        let (mime, bytes) = decode_data_url("data:image/svg+xml,%3Csvg%3E").unwrap();
        assert_eq!((mime.as_str(), bytes.as_slice()), ("image/svg+xml", &b"<svg>"[..]));
        assert!(decode_data_url("https://x.test/a.png").is_none());
        assert!(decode_data_url("data:nocomma").is_none());
    }

    #[test]
    fn base64_variants() {
        assert_eq!(base64_decode("aGVsbG8").unwrap(), b"hello");
        assert_eq!(base64_decode("+/-_").unwrap(), vec![0xfb, 0xff, 0xbf]);
    }

    #[test]
    fn images_and_sources() {
        assert!(decode_image("data:image/png;base64,iVBORw0KGgo=").is_some());
        assert!(decode_image("data:text/plain;base64,aGk=").is_none());
        assert_eq!(decode_image("data:image/svg+xml,%3Csvg%3E%3C/svg%3E").unwrap().bytes(), b"<svg></svg>");
        assert!(favicon_source(None).is_none());
        assert!(favicon_source(Some("https://a.com/f.ico")).is_some());
        assert!(favicon_source(Some("ftp://a.com/f.ico")).is_none());
        assert!(favicon_source(Some("data:image/unknown;base64,AAAA")).is_none());
    }
}
