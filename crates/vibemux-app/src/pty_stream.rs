use std::collections::VecDeque;
use std::io::Read;
use std::sync::{Arc, Mutex};

pub struct PtyReader {
    buffer: Arc<Mutex<VecDeque<Vec<u8>>>>,
    alive: Arc<Mutex<bool>>,
}

impl PtyReader {
    pub fn spawn(reader: Arc<Mutex<Box<dyn Read + Send>>>) -> Self {
        let buffer: Arc<Mutex<VecDeque<Vec<u8>>>> =
            Arc::new(Mutex::new(VecDeque::new()));
        let alive = Arc::new(Mutex::new(true));

        let buf_clone = Arc::clone(&buffer);
        let alive_clone = Arc::clone(&alive);

        std::thread::spawn(move || {
            let mut read_buf = [0u8; 8192];
            loop {
                let n = {
                    let mut r = reader.lock().unwrap();
                    match r.read(&mut read_buf) {
                        Ok(n) => n,
                        Err(_) => break,
                    }
                };

                if n == 0 {
                    break;
                }
                {
                    let mut buf = buf_clone.lock().unwrap();
                    buf.push_back(read_buf[..n].to_vec());
                }
            }

            *alive_clone.lock().unwrap() = false;
        });

        Self { buffer, alive }
    }

    pub fn drain(&self) -> Vec<u8> {
        let mut buf = self.buffer.lock().unwrap();
        let mut result = Vec::new();
        while let Some(chunk) = buf.pop_front() {
            result.extend_from_slice(&chunk);
        }
        result
    }

    pub fn is_alive(&self) -> bool {
        *self.alive.lock().unwrap()
    }
}
