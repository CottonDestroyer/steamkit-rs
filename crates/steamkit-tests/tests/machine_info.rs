//! `HardwareUtils` machine info tests.
//!
//! * Rust has no GC, so the "provider is not retained" test drops the last
//!   `Arc` and checks the weak reference the generation table holds.
//! * `IMachineInfoProvider` returns `Option<Vec<u8>>` and can't throw, so the
//!   "exception bubbles up" test uses a provider that panics and checks that
//!   the panic reaches the caller with the same message.
//! * The thread safety test releases all threads at once with a `Barrier`.

use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use steamkit::steam::steam_client::configuration::i_machine_info_provider::IMachineInfoProvider;
use steamkit::types::key_value::KeyValue;
use steamkit::util::hardware_utils::{HardwareUtils, MachineInfoProvider};

/// The number of threads the thread safety test starts.
const NUM_THREADS: usize = 100;

/// Counts every call to the provider.
struct CountingMachineInfoProvider {
    total_invocations: AtomicUsize,
}

impl CountingMachineInfoProvider {
    fn new() -> CountingMachineInfoProvider {
        CountingMachineInfoProvider {
            total_invocations: AtomicUsize::new(0),
        }
    }

    fn total_invocations(&self) -> usize {
        self.total_invocations.load(Ordering::SeqCst)
    }
}

impl IMachineInfoProvider for CountingMachineInfoProvider {
    fn machine_guid(&self) -> Option<Vec<u8>> {
        self.total_invocations.fetch_add(1, Ordering::SeqCst);
        Some(Vec::new())
    }

    fn mac_address(&self) -> Option<Vec<u8>> {
        self.total_invocations.fetch_add(1, Ordering::SeqCst);
        Some(Vec::new())
    }

    fn disk_id(&self) -> Option<Vec<u8>> {
        self.total_invocations.fetch_add(1, Ordering::SeqCst);
        Some(Vec::new())
    }
}

/// A provider that refuses to run on one particular thread.
struct ThreadRejectingMachineInfoProvider {
    thread_id_to_reject: std::thread::ThreadId,
}

impl ThreadRejectingMachineInfoProvider {
    fn new(thread_id_to_reject: std::thread::ThreadId) -> ThreadRejectingMachineInfoProvider {
        ThreadRejectingMachineInfoProvider { thread_id_to_reject }
    }

    fn ensure_not_on_rejected_thread(&self) {
        assert_ne!(
            std::thread::current().id(),
            self.thread_id_to_reject,
            "Operation must not be run on rejected thread."
        );
    }
}

impl IMachineInfoProvider for ThreadRejectingMachineInfoProvider {
    fn machine_guid(&self) -> Option<Vec<u8>> {
        self.ensure_not_on_rejected_thread();
        Some(Vec::new())
    }

    fn mac_address(&self) -> Option<Vec<u8>> {
        self.ensure_not_on_rejected_thread();
        Some(Vec::new())
    }

    fn disk_id(&self) -> Option<Vec<u8>> {
        self.ensure_not_on_rejected_thread();
        Some(Vec::new())
    }
}

/// A provider that always returns the same values.
struct StaticMachineInfoProvider;

impl IMachineInfoProvider for StaticMachineInfoProvider {
    fn machine_guid(&self) -> Option<Vec<u8>> {
        Some("MachineGuid".as_bytes().to_vec())
    }

    fn mac_address(&self) -> Option<Vec<u8>> {
        Some("MacAddress".as_bytes().to_vec())
    }

    fn disk_id(&self) -> Option<Vec<u8>> {
        Some("DiskId".as_bytes().to_vec())
    }
}

/// A provider that only fails. The trait returns an `Option` and can't throw,
/// so the failure is a panic.
struct ThrowingMachineInfoProvider;

impl IMachineInfoProvider for ThrowingMachineInfoProvider {
    fn machine_guid(&self) -> Option<Vec<u8>> {
        panic!("This provider only throws.")
    }

    fn mac_address(&self) -> Option<Vec<u8>> {
        panic!("This provider only throws.")
    }

    fn disk_id(&self) -> Option<Vec<u8>> {
        panic!("This provider only throws.")
    }
}

#[test]
fn result_is_cached() {
    let counting = Arc::new(CountingMachineInfoProvider::new());
    let provider: Arc<dyn IMachineInfoProvider> = counting.clone();

    HardwareUtils::init(provider.clone());
    HardwareUtils::get_machine_id(&provider);

    let invocations = counting.total_invocations();

    for _ in 0..100 {
        HardwareUtils::init(provider.clone());
        HardwareUtils::get_machine_id(&provider);
    }

    assert_eq!(invocations, counting.total_invocations());
}

#[test]
fn result_is_cached_by_instance() {
    let counting = Arc::new(CountingMachineInfoProvider::new());
    let provider: Arc<dyn IMachineInfoProvider> = counting.clone();

    HardwareUtils::init(provider.clone());
    HardwareUtils::get_machine_id(&provider);

    let invocations = counting.total_invocations();

    for _ in 0..100 {
        let new_counting = Arc::new(CountingMachineInfoProvider::new());
        let new_provider: Arc<dyn IMachineInfoProvider> = new_counting.clone();
        assert_eq!(0, new_counting.total_invocations());

        HardwareUtils::init(new_provider.clone());
        HardwareUtils::get_machine_id(&new_provider);

        assert_eq!(invocations, new_counting.total_invocations());
        assert_eq!(invocations, counting.total_invocations());
    }

    assert_eq!(invocations, counting.total_invocations());
}

/// Generation runs on the calling thread if the background task hasn't
/// finished yet, so this gives the background generation a chance to
/// complete before asking for the machine id.
#[tokio::test(flavor = "multi_thread")]
async fn machine_info_is_processed_in_background() {
    let provider: Arc<dyn IMachineInfoProvider> =
        Arc::new(ThreadRejectingMachineInfoProvider::new(
            std::thread::current().id(),
        ));

    HardwareUtils::init(provider.clone());

    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    // Should not throw
    let _ = HardwareUtils::get_machine_id(&provider);
}

#[test]
fn default_provider_is_operative() {
    let default_provider = MachineInfoProvider::get_default_provider();

    HardwareUtils::init(default_provider.clone());
    let machine_id = HardwareUtils::get_machine_id(&default_provider);

    assert!(
        machine_id.is_some(),
        "the default provider should produce a machine id"
    );
}

/// The generation table only holds a `Weak` reference, so dropping the last
/// `Arc` must free the provider.
#[test]
fn provider_is_not_retained() {
    let counting = Arc::new(CountingMachineInfoProvider::new());
    let provider: Arc<dyn IMachineInfoProvider> = counting.clone();

    HardwareUtils::init(provider.clone());
    HardwareUtils::get_machine_id(&provider);

    let weak = Arc::downgrade(&counting);
    drop(counting);
    drop(provider);

    assert!(
        weak.upgrade().is_none(),
        "the provider should not be retained"
    );
}

#[test]
fn generation_is_thread_safe() {
    let counting = Arc::new(CountingMachineInfoProvider::new());
    let provider: Arc<dyn IMachineInfoProvider> = counting.clone();
    let trigger = std::sync::Barrier::new(NUM_THREADS);

    std::thread::scope(|scope| {
        let trigger = &trigger;
        let provider = &provider;

        for _ in 0..NUM_THREADS {
            scope.spawn(move || {
                trigger.wait();

                HardwareUtils::init(provider.clone());
                HardwareUtils::get_machine_id(provider);
            });
        }
    });

    assert_eq!(3, counting.total_invocations());
}

#[test]
fn generates_message_object() {
    let provider: Arc<dyn IMachineInfoProvider> = Arc::new(StaticMachineInfoProvider);

    HardwareUtils::init(provider.clone());

    let message_object_data = HardwareUtils::get_machine_id(&provider)
        .expect("the machine id should have been generated");

    let mut kv = KeyValue::new();
    let read = kv.try_read_as_binary(&mut Cursor::new(message_object_data));

    assert!(read);

    assert_eq!(Some("MessageObject"), kv.name());
    assert_eq!(
        Some("3018ba91fc5a72f8b3f74501af6dd6da331b6cbc"),
        kv.get("BB3").as_string()
    );
    assert_eq!(
        Some("5d7e7734714b64bd6d88fef3ddbbf7ab4a749c5e"),
        kv.get("FF2").as_string()
    );
    assert_eq!(
        Some("a1807176456746ba2a3f5574bf47677a919dab49"),
        kv.get("3B3").as_string()
    );
}

/// The provider trait returns `Option` and `get_machine_id` swallows a failed
/// generation, so a provider that only fails has to panic. The panic must
/// reach the caller with the same message.
#[test]
fn exception_bubbles_up() {
    let provider: Arc<dyn IMachineInfoProvider> = Arc::new(ThrowingMachineInfoProvider);

    HardwareUtils::init(provider.clone());

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        HardwareUtils::get_machine_id(&provider)
    }));

    match result {
        Ok(value) => panic!("the provider should have thrown, but it returned {value:?}"),
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("");

            assert_eq!("This provider only throws.", message);
        }
    }
}

