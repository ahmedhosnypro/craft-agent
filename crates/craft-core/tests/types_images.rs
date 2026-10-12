//! Image attachments: ImagePart round-trips and the images field keeps
//! the image-less serialized shape byte-identical.

use craft_core::types::{ChatMessage, ImagePart};

#[test]
fn base64_and_url_image_parts_round_trip() -> Result<(), serde_json::Error> {
    let message = ChatMessage::User {
        text: String::from("what is in this picture?"),
        images: vec![
            ImagePart::Base64 {
                media_type: String::from("image/png"),
                data: String::from("aGk="),
            },
            ImagePart::Url {
                url: String::from("https://example.test/cat.png"),
            },
        ],
    };
    let json = serde_json::to_string(&message)?;
    assert!(json.contains("\"images\""), "images must serialize: {json}");
    assert!(json.contains("\"kind\":\"base64\""));
    assert!(json.contains("\"kind\":\"url\""));
    let back: ChatMessage = serde_json::from_str(&json)?;
    assert_eq!(back, message);
    Ok(())
}

#[test]
fn tool_message_images_round_trip() -> Result<(), serde_json::Error> {
    let message = ChatMessage::Tool {
        call_id: String::from("c1"),
        content: String::from("the screenshot shows a crash"),
        is_error: false,
        images: vec![ImagePart::Base64 {
            media_type: String::from("image/jpeg"),
            data: String::from("aGk="),
        }],
    };
    let json = serde_json::to_string(&message)?;
    assert!(json.contains("\"images\""), "images must serialize: {json}");
    let back: ChatMessage = serde_json::from_str(&json)?;
    assert_eq!(back, message);
    Ok(())
}

#[test]
fn imageless_user_serializes_byte_identical_to_the_old_shape() -> Result<(), serde_json::Error> {
    let json = serde_json::to_string(&ChatMessage::User {
        text: String::from("hi"),
        images: Vec::new(),
    })?;
    assert_eq!(json, r#"{"role":"user","text":"hi"}"#);
    Ok(())
}

#[test]
fn imageless_tool_serializes_byte_identical_to_the_old_shape() -> Result<(), serde_json::Error> {
    let json = serde_json::to_string(&ChatMessage::Tool {
        call_id: String::from("c1"),
        content: String::from("out"),
        is_error: false,
        images: Vec::new(),
    })?;
    assert_eq!(
        json,
        r#"{"role":"tool","call_id":"c1","content":"out","is_error":false}"#
    );
    Ok(())
}

#[test]
fn records_without_the_images_field_still_deserialize() -> Result<(), serde_json::Error> {
    let user: ChatMessage = serde_json::from_str(r#"{"role":"user","text":"hi"}"#)?;
    assert_eq!(
        user,
        ChatMessage::User {
            text: String::from("hi"),
            images: Vec::new(),
        }
    );
    let tool: ChatMessage =
        serde_json::from_str(r#"{"role":"tool","call_id":"c1","content":"out","is_error":false}"#)?;
    assert_eq!(
        tool,
        ChatMessage::Tool {
            call_id: String::from("c1"),
            content: String::from("out"),
            is_error: false,
            images: Vec::new(),
        }
    );
    Ok(())
}
