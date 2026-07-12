use super::{select_format, ClipboardContent, ContentFormat, ContentKind};

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn file_uri_wins_over_image_and_plain_text_representations() {
    let format = select_format(&strings(&[
        "text/plain;charset=utf-8",
        "image/png",
        "x-special/gnome-copied-files",
        "text/uri-list",
    ]))
    .unwrap();

    assert_eq!(format.kind, ContentKind::Files);
    assert_eq!(format.mime_type, "text/uri-list");
}

#[test]
fn image_video_and_explicit_utf8_formats_are_selected() {
    let image = select_format(&strings(&["image/jpeg", "image/png"])).unwrap();
    assert_eq!(image.kind, ContentKind::Image);
    assert_eq!(image.mime_type, "image/png");

    let video = select_format(&strings(&["application/octet-stream", "video/webm"])).unwrap();
    assert_eq!(video.kind, ContentKind::Video);

    let text = select_format(&strings(&["text/html", "text/plain;charset=utf-8"])).unwrap();
    assert_eq!(text.kind, ContentKind::Text);
    assert_eq!(text.mime_type, "text/plain;charset=utf-8");
}

#[test]
fn unsupported_and_portal_only_formats_are_ignored() {
    assert!(select_format(&strings(&["application/octet-stream"])).is_none());
    assert!(select_format(&strings(&["application/vnd.portal.filetransfer"])).is_none());
}

#[test]
fn text_and_file_payloads_require_utf8_but_media_preserves_arbitrary_bytes() {
    let text = ContentFormat {
        kind: ContentKind::Text,
        mime_type: "text/plain".to_owned(),
    };
    assert!(ClipboardContent::new(text.clone(), vec![0xff]).is_none());
    assert_eq!(
        ClipboardContent::new(text, "中文".as_bytes().to_vec())
            .unwrap()
            .bytes,
        "中文".as_bytes()
    );

    let image = ContentFormat {
        kind: ContentKind::Image,
        mime_type: "image/png".to_owned(),
    };
    assert_eq!(
        ClipboardContent::new(image, vec![0x00, 0xff])
            .unwrap()
            .bytes,
        [0x00, 0xff]
    );
}

#[test]
fn file_payload_exposes_decoded_names_and_safe_copy_uri_list() {
    let content = ClipboardContent::new(
        ContentFormat {
            kind: ContentKind::Files,
            mime_type: "x-special/gnome-copied-files".to_owned(),
        },
        b"cut\nfile:///home/user/%E8%A7%86%E9%A2%91%20demo.mp4\nfile:///tmp/image.png\n".to_vec(),
    )
    .unwrap();

    assert_eq!(content.display_title(), "视频 demo.mp4 · image.png");
    assert!(content.search_text().contains("视频 demo.mp4"));
    assert_eq!(
        content.uri_list_bytes().unwrap(),
        b"file:///home/user/%E8%A7%86%E9%A2%91%20demo.mp4\r\nfile:///tmp/image.png\r\n"
    );
}

#[test]
fn equivalent_file_representations_share_a_fingerprint() {
    let uri_list = ClipboardContent::new(
        ContentFormat {
            kind: ContentKind::Files,
            mime_type: "text/uri-list".to_owned(),
        },
        b"file:///tmp/demo.mp4\r\n".to_vec(),
    )
    .unwrap();
    let gnome = ClipboardContent::new(
        ContentFormat {
            kind: ContentKind::Files,
            mime_type: "x-special/gnome-copied-files".to_owned(),
        },
        b"copy\nfile:///tmp/demo.mp4\n".to_vec(),
    )
    .unwrap();

    assert_eq!(uri_list.fingerprint(), gnome.fingerprint());
}

#[test]
fn malformed_file_lists_are_rejected() {
    assert!(ClipboardContent::new(
        ContentFormat {
            kind: ContentKind::Files,
            mime_type: "text/uri-list".to_owned(),
        },
        b"copy\nnot a URI\n".to_vec(),
    )
    .is_none());
}
