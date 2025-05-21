use std::net::TcpListener;
use std::io::{Read, BufReader};

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:5005")?;
    println!("Listening on port 5005...");

    for stream in listener.incoming() {
        let mut stream = stream?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut buffer = [0; 1024];
        loop {
            let n = reader.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            let text = String::from_utf8_lossy(&buffer[..n]);
            println!("Received: {}", text);
        }
    }

    Ok(())
}
