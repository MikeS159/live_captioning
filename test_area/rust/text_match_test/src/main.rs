use rapidfuzz::distance::levenshtein;
use rapidfuzz::distance::prefix;
use std::fs::File;
use std::io::{self, BufRead};
use std::collections::BinaryHeap;
use ordered_float::OrderedFloat;
use std::cmp::{Ordering, Reverse};
use textdistance::{Algorithm, SorensenDice};

#[derive(PartialEq, PartialOrd, Debug)]
struct OrderedF64(f64);

impl Eq for OrderedF64 {}

impl Ord for OrderedF64 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

fn main() -> io::Result<()> {
    let file = File::open("Conversation.txt")?;
    let reader = io::BufReader::new(file);

    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;
    let mut heap = BinaryHeap::new();

    let s = "You mean the bimpy";// code? The binary multiply?";
    for line in &lines {
        let a = SorensenDice::default();
        let r = a.for_str(s, line);
        //println!("{} - {}", r.nval(), line);
        heap.push(Reverse((OrderedF64(r.nval()), line)));
        //println!("{} - {}", l, line);
    }

    // Print in descending order (BinaryHeap is a max heap by default)
    while let Some(Reverse((OrderedF64(l), line))) = heap.pop() {
        println!("{}: {}", l, line);
    }

    Ok(())
}

