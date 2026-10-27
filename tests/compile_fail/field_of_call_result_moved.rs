use assertoscope::assert;

#[derive(Debug)]
struct Report {
    body: Option<String>,
    items: Vec<u32>,
}

fn fetch() -> Report {
    Report {
        body: Some("text".to_owned()),
        items: vec![1, 2],
    }
}

fn main() {
    // A field of a call result is recorded through a reference: moving it
    // out, or borrowing it mutably, does not compile.
    assert!(fetch().body.unwrap() == "text");
    assert!(fetch().items.pop() == Some(2));
}
