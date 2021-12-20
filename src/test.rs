#![cfg(test)]

use crate::Vechonk;
use alloc::boxed::Box;

#[test]
fn new() {
    let chonk = Vechonk::<()>::new();

    assert_eq!(chonk.len(), 0);
}

#[test]
fn zero_capacity() {
    let chonk = Vechonk::<()>::with_capacity(0);

    assert_eq!(chonk.len(), 0);
}

#[test]
fn some_capacity() {
    let chonk = Vechonk::<()>::with_capacity(96);

    assert_eq!(chonk.len(), 0);
}

#[test]
fn push_single_sized_elem() {
    let mut chonk = Vechonk::<u8>::with_capacity(96);

    chonk.push(Box::new(1));

    assert_eq!(chonk.len(), 1);
}

#[test]
fn push_single_unsized_elem() {
    let mut chonk = Vechonk::<str>::with_capacity(96);

    chonk.push("hello".into());

    assert_eq!(chonk.len(), 1);
}

#[test]
fn push_two_sized_elem() {
    let mut chonk = Vechonk::<u8>::with_capacity(96);

    chonk.push(Box::new(1));
    chonk.push(Box::new(2));

    assert_eq!(chonk.len(), 2);
    assert_eq!(chonk.elem_size, 2);
    assert_eq!(chonk.data_section_size(), 16); // two indecies
}

#[test]
fn push_two_unsized_elem() {
    let mut chonk = Vechonk::<str>::with_capacity(96);

    chonk.push("hello".into());
    chonk.push("uwu".into());

    assert_eq!(chonk.len(), 2);
    assert_eq!(chonk.elem_size, 8);
    assert_eq!(chonk.data_section_size(), 32); // two indecies + lengths
}

#[test]
#[should_panic]
fn index_out_of_bounds() {
    let chonk = Vechonk::<str>::with_capacity(96);

    let _ = chonk[0];
}

#[test]
fn index() {
    let mut chonk = Vechonk::<str>::with_capacity(96);

    chonk.push("hello".into());
    chonk.push("uwu".into());

    let hello = &chonk[0];
    let uwu = &chonk[1];

    assert_eq!(hello, "hello");
    assert_eq!(uwu, "uwu");
}
