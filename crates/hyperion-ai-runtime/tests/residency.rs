//! docs/22-local-ai-runtime.md §5.2: evict the least valuable `Cold`
//! candidate when a new model doesn't fit; `pin_count` overrides eviction.

use hyperion_ai_runtime::{
    LocalAiRuntime, MockBackend, ModelClass, ModelDescriptor, Precision, QuantizedVariant,
    ResidencyStatus,
};

fn descriptor(model_id: u64, footprint_mb: u32) -> ModelDescriptor {
    let mut d = ModelDescriptor {
        model_id,
        class: ModelClass::Slm,
        variants: vec![QuantizedVariant {
            precision: Precision::Fp16,
            footprint_mb,
            expected_tokens_per_sec: 50.0,
        }],
        checksum: 0,
    };
    d.checksum = hyperion_ai_runtime::checksum(&d);
    d
}

#[test]
fn loading_a_second_model_evicts_the_least_valuable_resident() {
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 1_500);
    let a = descriptor(1, 1_000);
    let b = descriptor(2, 1_000);
    runtime.register_model(a.clone()).unwrap();
    runtime.register_model(b.clone()).unwrap();

    runtime.load(a.model_id, &a.variants[0]).unwrap();
    assert_eq!(
        runtime.residency_of(a.model_id).unwrap().status,
        ResidencyStatus::Hot
    );

    // b doesn't fit alongside a (1000 + 1000 > 1500); a must be evicted.
    runtime.load(b.model_id, &b.variants[0]).unwrap();
    assert_eq!(
        runtime.residency_of(a.model_id).unwrap().status,
        ResidencyStatus::Cold
    );
    assert_eq!(
        runtime.residency_of(b.model_id).unwrap().status,
        ResidencyStatus::Hot
    );
}

#[test]
fn pinned_model_is_never_evicted() {
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 1_500);
    let a = descriptor(1, 1_000);
    let b = descriptor(2, 1_000);
    runtime.register_model(a.clone()).unwrap();
    runtime.register_model(b.clone()).unwrap();

    runtime.load(a.model_id, &a.variants[0]).unwrap();
    runtime.pin(a.model_id);

    let result = runtime.load(b.model_id, &b.variants[0]);
    assert!(
        result.is_err(),
        "b cannot load: a is pinned and there's nothing else to evict"
    );
    assert_eq!(
        runtime.residency_of(a.model_id).unwrap().status,
        ResidencyStatus::Hot,
        "pinned model must survive the failed eviction attempt"
    );
}

#[test]
fn reloading_an_already_hot_model_is_a_cheap_touch_not_a_reload() {
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 1_000);
    let a = descriptor(1, 1_000);
    runtime.register_model(a.clone()).unwrap();

    runtime.load(a.model_id, &a.variants[0]).unwrap();
    runtime.load(a.model_id, &a.variants[0]).unwrap();
    assert_eq!(
        runtime.residency_of(a.model_id).unwrap().status,
        ResidencyStatus::Hot
    );
}

#[test]
fn higher_predicted_next_use_survives_eviction_over_a_less_valuable_resident() {
    let runtime = LocalAiRuntime::new(Box::new(MockBackend), 2_000);
    let a = descriptor(1, 1_000);
    let b = descriptor(2, 1_000);
    let c = descriptor(3, 1_000);
    for d in [&a, &b, &c] {
        runtime.register_model(d.clone()).unwrap();
    }

    runtime.load(a.model_id, &a.variants[0]).unwrap();
    runtime.load(b.model_id, &b.variants[0]).unwrap();
    // Both a and b resident now (2000mb used, at capacity). Mark a as
    // predicted to be used again soon; b is not.
    runtime.set_predicted_next_use(a.model_id, 1.0);
    runtime.set_predicted_next_use(b.model_id, 0.0);

    runtime.load(c.model_id, &c.variants[0]).unwrap();
    assert_eq!(
        runtime.residency_of(b.model_id).unwrap().status,
        ResidencyStatus::Cold,
        "b had the lowest predicted-next-use value and should be evicted"
    );
    assert_eq!(
        runtime.residency_of(a.model_id).unwrap().status,
        ResidencyStatus::Hot
    );
}
