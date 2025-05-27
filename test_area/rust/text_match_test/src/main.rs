use std::fs::File;
use std::io::{self, BufRead};
use std::collections::BinaryHeap;
use std::cmp::{Ordering};
use textdistance::{Algorithm, SorensenDice};
use std::time::Instant;

#[derive(PartialEq, PartialOrd, Debug)]
struct OrderedF64(f64);

impl Eq for OrderedF64 {}

impl Ord for OrderedF64 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

fn main() -> io::Result<()> {
    let script_file = File::open("C:\\Users\\u290676\\Repos\\Github\\live_captioning\\test_area\\script_noname.txt")?;
    let gen_file = File::open("C:\\Users\\u290676\\Repos\\Github\\live_captioning\\test_area\\output_wav.txt")?;
    let script_reader = io::BufReader::new(script_file);
    let gen_reader = io::BufReader::new(gen_file);

    let script_lines: Vec<String> = script_reader.lines().collect::<Result<_, _>>()?;
    let gen_lines: Vec<String> = gen_reader.lines().collect::<Result<_, _>>()?;
    let mut heap = BinaryHeap::new();

    let start = Instant::now();

    for gen_line in &gen_lines {
        //println!("Searching for line - {}", gen_line);
        for script_line in &script_lines {
            let a = SorensenDice::default();
            let r = a.for_str(gen_line, script_line);
            //println!("{} - {}", r.nval(), line);
            heap.push((OrderedF64(r.nval()), script_line));
        }
        // Print in descending order (BinaryHeap is a max heap by default)
        for _ in 0..5 {
            if let Some((OrderedF64(l), script_line)) = heap.pop() {
                //println!("{}: {}", l, script_line);
            } else {
                break;
            }
        }
        //let mut line = String::new();
        //std::io::stdin().read_line(&mut line)?;
    }
    let duration = start.elapsed();
    println!("Execution time: {:.2?}", duration);
    Ok(())
}

