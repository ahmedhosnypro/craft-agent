//! Image attachments: block-array content with a whitelisted base64
//! media type or a url source.

use craft_core::error::ProviderError;
use craft_core::types::ImagePart;
use serde_json::Value;

/// The base64 media types the Messages API documents.
const MEDIA_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Message-level content: the text block, then one block per image;
/// content stays a plain string when no images ride.
pub(super) fn with_images(text: &str, images: &[ImagePart]) -> Result<Value, ProviderError> {
    if images.is_empty() {
        return Ok(Value::String(text.to_owned()));
    }
    let mut blocks = Vec::with_capacity(images.len() + 1);
    if !text.is_empty() {
        blocks.push(serde_json::json!({"type": "text", "text": text}));
    }
    for part in images {
        blocks.push(block(part)?);
    }
    Ok(Value::Array(blocks))
}

fn block(part: &ImagePart) -> Result<Value, ProviderError> {
    match part {
        ImagePart::Base64 { media_type, data } => {
            if !MEDIA_TYPES.contains(&media_type.as_str()) {
                return Err(ProviderError::InvalidRequest {
                    detail: format!("unsupported image media_type: {media_type}"),
                });
            }
            Ok(serde_json::json!({
                "type": "image",
                "source": {"type": "base64", "media_type": media_type, "data": data},
            }))
        }
        ImagePart::Url { url } => Ok(serde_json::json!({
            "type": "image",
            "source": {"type": "url", "url": url},
        })),
    }
}
