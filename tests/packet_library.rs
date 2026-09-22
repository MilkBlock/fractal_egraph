use egg_layout::packet_library::{Effect, Library, Packet};
fn packet(a: usize, b: usize) -> Packet {
    Packet::normalize(
        "comm".into(),
        vec![
            Effect::Ensure {
                op: "Add".into(),
                args: vec![b, a],
                result: a,
            },
            Effect::Equate(a, b),
        ],
    )
}
#[test]
fn aliases_and_binding_survive_normalization() {
    let a = packet(10, 20);
    let b = packet(30, 40);
    assert_eq!(a.template, b.template);
    assert_ne!(a.binding, b.binding);
    assert_ne!(a.template, packet(10, 10).template);
    let mut lib = Library::default();
    lib.packet(a);
    lib.packet(b);
    assert_eq!(lib.templates.len(), 1);
}
#[test]
fn sequence_hash_is_associative_but_ordered_and_slices_share() {
    let mut lib = Library::default();
    let a = lib.packet(packet(1, 2));
    let b = lib.packet(packet(3, 4));
    let c = lib.packet(packet(5, 6));
    let ab = lib.concat(a, b);
    let x = lib.concat(ab, c);
    let bc = lib.concat(b, c);
    let y = lib.concat(a, bc);
    assert_eq!(lib.nodes[x].hash, lib.nodes[y].hash);
    assert_eq!(lib.nodes[x].power, lib.nodes[y].power);
    let ba = lib.concat(b, a);
    assert_ne!(lib.nodes[ab].hash, lib.nodes[ba].hash);
    let n = lib.nodes.len();
    assert_eq!(lib.slice(x, 0, 2), Some(ab));
    assert_eq!(lib.nodes.len(), n);
    assert_eq!(
        lib.expand(x)
            .iter()
            .map(|p| p.binding.clone())
            .collect::<Vec<_>>(),
        vec![
            packet(1, 2).binding,
            packet(3, 4).binding,
            packet(5, 6).binding
        ]
    );
}
#[test]
fn binary_carries_bound_nodes_and_repeated_blocks_are_interned() {
    let mut lib = Library::default();
    let root = lib.build((0..4096).map(|_| packet(1, 2))).unwrap();
    assert_eq!(lib.nodes[root].len, 4096);
    assert_eq!(lib.nodes.len(), 13);
    let n = lib.nodes.len();
    assert_eq!(lib.build((0..4096).map(|_| packet(1, 2))), Some(root));
    assert_eq!(lib.nodes.len(), n);
    let mut varied = Library::default();
    let root = varied.build((0..257).map(|i| packet(i, i + 1))).unwrap();
    assert!(varied.nodes.len() < 2 * 257);
    let suffix = varied.slice(root, 129, 257).unwrap();
    assert_eq!(varied.expand(suffix).len(), 128);
}
