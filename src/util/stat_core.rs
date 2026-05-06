pub mod inner {
    use parking_lot::Mutex;
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    type AtomicCounter = Arc<AtomicU64>;
    type CounterMap = Arc<Mutex<HashMap<String, AtomicCounter>>>;
    type HistogramValues = Arc<Mutex<VecDeque<u64>>>;
    type HistogramMap = Arc<Mutex<HashMap<String, HistogramValues>>>;

    pub struct Metrics {
        counters: CounterMap,
        gauges: CounterMap,
        histograms: HistogramMap,
    }

    impl Metrics {
        pub fn new() -> Self {
            Self {
                counters: Arc::new(Mutex::new(HashMap::new())),
                gauges: Arc::new(Mutex::new(HashMap::new())),
                histograms: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        pub fn counter(&self, name: &str) -> Counter {
            let mut counters = self.counters.lock();
            let counter = counters
                .entry(name.to_string())
                .or_insert_with(|| Arc::new(AtomicU64::new(0)))
                .clone();
            Counter(counter)
        }

        pub fn gauge(&self, name: &str) -> Gauge {
            let mut gauges = self.gauges.lock();
            let gauge = gauges
                .entry(name.to_string())
                .or_insert_with(|| Arc::new(AtomicU64::new(0)))
                .clone();
            Gauge(gauge)
        }

        pub fn histogram(&self, name: &str) -> Histogram {
            let mut histograms = self.histograms.lock();
            let hist = histograms
                .entry(name.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(VecDeque::new())))
                .clone();
            Histogram(hist)
        }

        pub fn snapshot(&self) -> MetricsSnapshot {
            let counters = self.counters.lock();
            let gauges = self.gauges.lock();
            let histograms = self.histograms.lock();
            let counter_map: HashMap<String, u64> = counters
                .iter()
                .map(|(k, v)| (k.clone(), v.load(Ordering::Relaxed)))
                .collect();
            let gauge_map: HashMap<String, u64> = gauges
                .iter()
                .map(|(k, v)| (k.clone(), v.load(Ordering::Relaxed)))
                .collect();
            let hist_map: HashMap<String, Vec<u64>> = histograms
                .iter()
                .map(|(k, v)| (k.clone(), v.lock().iter().copied().collect()))
                .collect();
            MetricsSnapshot {
                counters: counter_map,
                gauges: gauge_map,
                histograms: hist_map,
            }
        }
    }

    impl Default for Metrics {
        fn default() -> Self {
            Self::new()
        }
    }

    pub struct Counter(Arc<AtomicU64>);

    impl Counter {
        pub fn inc(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }

        pub fn add(&self, v: u64) {
            self.0.fetch_add(v, Ordering::Relaxed);
        }
    }

    pub struct Gauge(Arc<AtomicU64>);

    impl Gauge {
        pub fn set(&self, v: u64) {
            self.0.store(v, Ordering::Relaxed);
        }

        pub fn inc(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }

        pub fn dec(&self) {
            self.0.fetch_sub(1, Ordering::Relaxed);
        }
    }

    pub struct Histogram(Arc<Mutex<VecDeque<u64>>>);

    impl Histogram {
        pub fn record(&self, v: u64) {
            let mut vec = self.0.lock();
            vec.push_back(v);
            if vec.len() > 1000 {
                vec.pop_front();
            }
        }
    }

    #[derive(Debug, Clone)]
    pub struct MetricsSnapshot {
        pub counters: HashMap<String, u64>,
        pub gauges: HashMap<String, u64>,
        pub histograms: HashMap<String, Vec<u64>>,
    }

    static METRICS: std::sync::LazyLock<Metrics> = std::sync::LazyLock::new(Metrics::new);

    pub fn metrics() -> &'static Metrics {
        &METRICS
    }
}
