use std::io::BufWriter;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dump <file.replay>");
    let bytes = std::fs::read(&path).expect("read replay");
    let schema = chests::fortnite_schema();
    let stdout = std::io::stdout();
    let mut dump = chests::dump::EventDump::new(BufWriter::new(stdout.lock()));
    let started = std::time::Instant::now();
    let replay = fnreplay::parse(&bytes, &schema, &mut dump).expect("parse");
    eprintln!(
        "engine={} network={} packets={} failed={} updates={} broken_chunks={} in {:?}",
        replay.header.engine_network_version,
        replay.header.network_version,
        replay.stream.packets,
        replay.stream.failed_packets,
        replay.stream.updates,
        replay.broken_chunks,
        started.elapsed()
    );
}
