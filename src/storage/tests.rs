use rusqlite::Connection;

use crate::content::{ClipboardContent, ContentFormat, ContentKind};

use super::{RecordOutcome, Storage};

fn storage(max_items: usize) -> (tempfile::TempDir, Storage) {
    let temp = tempfile::tempdir().unwrap();
    let store = Storage::open(&temp.path().join("history.sqlite3"), max_items).unwrap();
    (temp, store)
}

fn content(kind: ContentKind, mime_type: &str, bytes: &[u8]) -> ClipboardContent {
    ClipboardContent::new(
        ContentFormat {
            kind,
            mime_type: mime_type.to_owned(),
        },
        bytes.to_vec(),
    )
    .unwrap()
}

#[test]
fn stores_binary_exactly_and_lists_only_media_metadata() {
    let (_temp, mut store) = storage(10);
    let image = content(ContentKind::Image, "image/png", &[0x00, 0xff, 0x42]);
    let id = match store.record(&image, Some(b"tiny-png")).unwrap() {
        RecordOutcome::Inserted(id) => id,
        RecordOutcome::IgnoredBlank => unreachable!(),
    };

    let items = store.list().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, ContentKind::Image);
    assert_eq!(items[0].mime_type, "image/png");
    assert_eq!(items[0].byte_len, 3);
    assert_eq!(items[0].thumbnail.as_deref(), Some(b"tiny-png".as_slice()));
    assert_eq!(store.get_content(id).unwrap().unwrap(), image);
}

#[test]
fn duplicate_moves_to_front_while_same_bytes_with_other_mime_stays_distinct() {
    let (_temp, mut store) = storage(3);
    let png = content(ContentKind::Image, "image/png", b"same");
    let video = content(ContentKind::Video, "video/mp4", b"same");
    store.record(&png, None).unwrap();
    store.record(&video, None).unwrap();
    store.record(&png, None).unwrap();

    let items = store.list().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].kind, ContentKind::Image);
    assert_eq!(items[1].kind, ContentKind::Video);
}

#[test]
fn migrates_legacy_text_rows_without_losing_exact_content() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("history.sqlite3");
    let legacy = Connection::open(&path).unwrap();
    legacy
        .execute_batch(
            "CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                content_hash BLOB NOT NULL UNIQUE,
                created_at_ms INTEGER NOT NULL
            );",
        )
        .unwrap();
    let text = "旧历史\n逐字保留";
    let hash = blake3::hash(text.as_bytes());
    legacy
        .execute(
            "INSERT INTO history(content, content_hash, created_at_ms) VALUES (?1, ?2, 7)",
            rusqlite::params![text, hash.as_bytes().as_slice()],
        )
        .unwrap();
    drop(legacy);

    let store = Storage::open(&path, 10).unwrap();
    let item = store.list().unwrap().pop().unwrap();
    assert_eq!(item.kind, ContentKind::Text);
    assert_eq!(item.search_text, text);
    let restored = store.get_content(item.id).unwrap().unwrap();
    assert_eq!(restored.bytes, text.as_bytes());
    assert_eq!(restored.mime_type, "text/plain;charset=utf-8");
}

#[test]
fn delete_clear_and_pause_are_persistent() {
    let (temp, mut store) = storage(10);
    let secret = content(ContentKind::Text, "text/plain", b"secret");
    let id = match store.record(&secret, None).unwrap() {
        RecordOutcome::Inserted(id) => id,
        RecordOutcome::IgnoredBlank => unreachable!(),
    };
    assert!(store.delete(id).unwrap());
    assert!(!store.delete(id).unwrap());
    store
        .record(&content(ContentKind::Text, "text/plain", b"one"), None)
        .unwrap();
    store
        .record(&content(ContentKind::Text, "text/plain", b"two"), None)
        .unwrap();
    assert_eq!(store.clear().unwrap(), 2);
    store.set_paused(true).unwrap();
    drop(store);

    let reopened = Storage::open(&temp.path().join("history.sqlite3"), 10).unwrap();
    assert!(reopened.is_paused().unwrap());
    assert_eq!(reopened.count().unwrap(), 0);
}
