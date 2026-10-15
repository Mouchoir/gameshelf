//! Artwork cache: downloads an image once, resizes it for the grid, stores it by content hash.

use image::{ImageFormat, imageops::FilterType};
use sha2::{Digest, Sha256};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

/// Maximum size of a downloaded image.
const MAX_BYTES: usize = 15 * 1024 * 1024;
/// Grid covers are stored at most this large (2:3 portrait, enough for 2x screens).
const GRID_W: u32 = 600;
const GRID_H: u32 = 900;
const JPEG_QUALITY: u8 = 85;

#[derive(Debug, thiserror::Error)]
pub enum ArtworkError {
    #[error("image not found")]
    NotFound,
    #[error("download failed: {0}")]
    Http(String),
    #[error("not a supported image")]
    Decode,
    #[error("cannot write the artwork cache: {0}")]
    Io(#[from] std::io::Error),
}

/// A cached image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredImage {
    pub sha256: String,
    pub mime: &'static str,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug)]
pub struct ArtworkCache {
    dir: PathBuf,
    http: reqwest::Client,
}

impl ArtworkCache {
    pub fn new(dir: PathBuf, http: reqwest::Client) -> Self {
        Self { dir, http }
    }

    /// Path of a cached file.
    pub fn path(&self, sha256: &str) -> PathBuf {
        self.dir.join(sha256)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Downloads, resizes to the grid size and stores an image.
    pub async fn fetch(&self, url: &str) -> Result<StoredImage, ArtworkError> {
        let resp = self
            .http
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| ArtworkError::Http(e.without_url().to_string()))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ArtworkError::NotFound);
        }
        if !resp.status().is_success() {
            return Err(ArtworkError::Http(format!("status {}", resp.status().as_u16())));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ArtworkError::Http(e.without_url().to_string()))?;
        if bytes.len() > MAX_BYTES {
            return Err(ArtworkError::Decode);
        }
        self.store_bytes(bytes.to_vec()).await
    }

    /// Resizes and stores raw image bytes.
    pub async fn store_bytes(&self, bytes: Vec<u8>) -> Result<StoredImage, ArtworkError> {
        let (data, width, height) = tokio::task::spawn_blocking(move || resize(&bytes))
            .await
            .map_err(|_| ArtworkError::Decode)??;
        let sha256 = hex(&Sha256::digest(&data));
        tokio::fs::create_dir_all(&self.dir).await?;
        let path = self.path(&sha256);
        if tokio::fs::metadata(&path).await.is_err() {
            let tmp = self.dir.join(format!("{sha256}.tmp"));
            tokio::fs::write(&tmp, &data).await?;
            tokio::fs::rename(&tmp, &path).await?;
        }
        Ok(StoredImage {
            sha256,
            mime: "image/jpeg",
            width,
            height,
        })
    }
}

fn resize(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), ArtworkError> {
    let img = image::load_from_memory(bytes).map_err(|_| ArtworkError::Decode)?;
    let img = if img.width() > GRID_W || img.height() > GRID_H {
        img.resize(GRID_W, GRID_H, FilterType::Lanczos3)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width(), rgb.height());
    let mut out = Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
    rgb.write_with_encoder(encoder)
        .map_err(|_| ArtworkError::Decode)?;
    let _ = ImageFormat::Jpeg;
    Ok((out.into_inner(), w, h))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([200, 40, 40]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).expect("png");
        out.into_inner()
    }

    #[tokio::test]
    async fn downloads_resizes_and_deduplicates() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cover.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(png(1200, 1800)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/missing.png"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().expect("dir");
        let cache = ArtworkCache::new(dir.path().join("assets"), reqwest::Client::new());
        let a = cache
            .fetch(&format!("{}/cover.png", server.uri()))
            .await
            .expect("fetch");
        assert_eq!((a.width, a.height), (600, 900));
        assert!(cache.path(&a.sha256).exists());
        let b = cache
            .fetch(&format!("{}/cover.png", server.uri()))
            .await
            .expect("again");
        assert_eq!(a.sha256, b.sha256);
        assert!(matches!(
            cache.fetch(&format!("{}/missing.png", server.uri())).await,
            Err(ArtworkError::NotFound)
        ));
        assert!(matches!(
            cache.store_bytes(b"not an image".to_vec()).await,
            Err(ArtworkError::Decode)
        ));
    }
}
