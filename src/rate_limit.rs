use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

pub const MAX_PER_IP: u32 = 5;
pub const MAX_PER_USER: u32 = 20;

const WINDOW: Duration = Duration::from_secs(15 * 60);

#[derive(Default)]
pub struct LoginLimiter {
    failures: Mutex<HashMap<String, (u32, Instant)>>,
}

impl LoginLimiter {
    pub fn check(&self, key: &str, max: u32) -> Result<(), Duration> {
        let failures = self.failures.lock().unwrap();

        match failures.get(key) {
            Some((count, since)) if *count >= max && since.elapsed() < WINDOW => {
                Err(WINDOW - since.elapsed())
            }
            _ => Ok(()),
        }
    }

    pub fn fail(&self, key: &str) {
        let mut failures = self.failures.lock().unwrap();

        failures.retain(|_, (_, since)| since.elapsed() < WINDOW);

        failures
            .entry(key.to_string())
            .or_insert((0, Instant::now()))
            .0 += 1;
    }

    pub fn clear(&self, key: &str) {
        self.failures.lock().unwrap().remove(key);
    }
}
