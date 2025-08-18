use super::*;

#[test]
fn test_basic_cache_spec() {
    let spec: CacheSpec = "64:100".parse().unwrap();
    assert_eq!(spec.line_size, 64);
    assert_eq!(spec.num_lines, 100);
}

#[test]
fn test_cache_spec_with_k() {
    let spec: CacheSpec = "4K:10".parse().unwrap();
    assert_eq!(spec.line_size, 4 * 1024);
    assert_eq!(spec.num_lines, 10);
}

#[test]
fn test_cache_spec_with_kib() {
    let spec: CacheSpec = "4KiB:10".parse().unwrap();
    assert_eq!(spec.line_size, 4 * 1024);
    assert_eq!(spec.num_lines, 10);
}

#[test]
fn test_cache_spec_with_mib() {
    let spec: CacheSpec = "1MiB:100".parse().unwrap();
    assert_eq!(spec.line_size, 1 * 1024 * 1024);
    assert_eq!(spec.num_lines, 100);
}

#[test]
fn test_multiple_cache_specs() {
    let specs: CacheSpecs = "64:100,4K:10,1MiB:100".parse().unwrap();
    assert_eq!(specs.specs.len(), 3);
    assert_eq!(specs.specs[0].line_size, 64);
    assert_eq!(specs.specs[0].num_lines, 100);
    assert_eq!(specs.specs[1].line_size, 4 * 1024);
    assert_eq!(specs.specs[1].num_lines, 10);
    assert_eq!(specs.specs[2].line_size, 1 * 1024 * 1024);
    assert_eq!(specs.specs[2].num_lines, 100);
}

// #[test]
// fn test_test_spec_parsing() {
//     assert!(matches!(
//         "btree".parse::<TestSpec>().unwrap(),
//         TestSpec::BTree
//     ));
//     assert!(matches!("veb".parse::<TestSpec>().unwrap(), TestSpec::VEB));
//     assert!(matches!("all".parse::<TestSpec>().unwrap(), TestSpec::All));
// }
