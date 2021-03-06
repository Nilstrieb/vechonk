# A `Vec<T: ?Sized>`

It's implemented by laying out the elements in memory contiguously like `alloc::vec::Vec`

# Layout

A `Vechonk` is 4 `usize` long. It owns a single allocation, containing the elements and the metadata.
The elements are laid out contiguously from the front, while the metadata is laid out contiguously from the back.
Both grow towards the center until they meet and get realloced to separate them again.

```txt

            Vechonk<str>
            ╭──────────────────────────────────╮
            │ ptr   | len   | cap  | elem_size │
            ╰──────────────────────────────────╯
               │               │        │
               │               ╰────────│──────────────────────────────────────╮
               │                        │                                      │
               │               ╭────────╯                                      │
        Heap   ▼               ▼                                               ▼
        ╭────────────┬─────────┬─────────────────┬──────────────┬──────────────╮
value   │ "hello"    │ "uwu"   │  <uninit>       │ 0 - 5        │ 5 - 3        │
        ├────────────┼─────────┼─────────────────┼──────────────┼──────────────┤
 size   │ dynamic    │ dynamic │  rest of alloc  │ usize + meta │ usize + meta │
        ╰────────────┴─────────┴─────────────────┴──────────────┴──────────────╯
            ▲            ▲                          │              │
            ╰────────────│──────────────────────────╯              │
                         ╰─────────────────────────────────────────╯
```