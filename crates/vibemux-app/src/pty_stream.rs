use std::collections::VecDeque;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct PtyReader {
    buffer: Arc<Mutex<VecDeque<Vec<u8>>>>,
    shutdown: Arc<AtomicBool>,
    /// Set to true when the reader thread detects new data, cleared by `drain`.
    has_data: Arc<AtomicBool>,
}

impl PtyReader {
    pub fn spawn(reader: Arc<Mutex<Box<dyn Read + Send>>>) -> Self {
        let buffer: Arc<Mutex<VecDeque<Vec<u8>>>> =
            Arc::new(Mutex::new(VecDeque::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let has_data = Arc::new(AtomicBool::new(false));

        let buf_clone = Arc::clone(&buffer);
        let shutdown_clone = Arc::clone(&shutdown);
        let has_data_clone = Arc::clone(&has_data);

        std::thread::spawn(move || {
            let mut read_buf = [0u8; 8192];
            loop {
                if shutdown_clone.load(Ordering::Relaxed) {
                    break;
                }
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
                    has_data_clone.store(true, Ordering::Relaxed);
                }
            }
        });

        Self {
            buffer,
            shutdown,
            has_data,
        }
    }

    /// Returns true if the reader thread has buffered data since the last `drain`.
    pub fn has_data(&self) -> bool {
        self.has_data.load(Ordering::Relaxed)
    }

    pub fn drain(&self) -> Vec<u8> {
        self.has_data.store(false, Ordering::Relaxed);
        let mut buf = self.buffer.lock().unwrap();
        let mut result = Vec::new();
        while let Some(chunk) = buf.pop_front() {
            result.extend_from_slice(&chunk);
        }
        result
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

impl Drop for PtyReader {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}
