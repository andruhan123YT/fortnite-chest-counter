//! Prints every player's opened and broken chest times as JSON, for diffing against
//! another replay parser.

struct Quiet;
impl fnreplay::Visitor for Quiet {}

fn main() {
    let path = std::env::args().nth(1).expect("usage: analyze <file.replay>");
    let bytes = std::fs::read(&path).expect("read replay");
    let (_, analysis) = chests::analyze_replay(&bytes, &chests::BreakConfig::default(), &mut Quiet).expect("parse");
    let mut out = serde_json::Map::new();
    for p in &analysis.players {
        if p.total() == 0 {
            continue;
        }
        let times = |events: &[chests::ChestEvent]| {
            events.iter().map(|e| (f64::from(e.time) * 10.0).round() / 10.0).collect::<Vec<_>>()
        };
        out.insert(p.name.clone(), serde_json::json!({"opened": times(&p.opened), "broken": times(&p.broken)}));
    }
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
