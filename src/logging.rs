use log::kv::{self, VisitSource};
use log::{Log, Metadata, Record};
use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_debug(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

struct PluginLogger;

impl Log for PluginLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        if DEBUG_ENABLED.load(Ordering::Relaxed) {
            metadata.level() <= log::Level::Debug
        } else {
            metadata.level() <= log::Level::Warn
        }
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut map = serde_json::Map::new();
        map.insert(
            "level".into(),
            serde_json::Value::String(record.level().as_str().into()),
        );
        map.insert(
            "message".into(),
            serde_json::Value::String(format!("{}", record.args())),
        );

        let mut kvs = KvCollector { map: &mut map };
        let _ = record.key_values().visit(&mut kvs);

        eprintln!("{}", serde_json::Value::Object(map));
    }

    fn flush(&self) {}
}

struct KvCollector<'a> {
    map: &'a mut serde_json::Map<String, serde_json::Value>,
}

impl<'a, 'kvs> VisitSource<'kvs> for KvCollector<'a> {
    fn visit_pair(
        &mut self,
        key: kv::Key<'kvs>,
        value: kv::Value<'kvs>,
    ) -> Result<(), kv::Error> {
        self.map.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
        Ok(())
    }
}

static LOGGER: PluginLogger = PluginLogger;

pub fn init() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Debug);
}
