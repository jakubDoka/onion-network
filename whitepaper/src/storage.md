# Storage Architecture

The architecture is based on [storj](https://www.storj.io/storjv3.pdf) with slight modifications. Following processes are more of a brainstorming of what data goes where when building up the storage mapping.

Fist lets define some terms:
- **piece**: single erasure code
- **block**: ordered sequence of pieces that is addressable by block-id
- **block-id**: unique identifier of a block (per satellite)
- **object**: sequence of bytes that is meaningful to the user
- **object address**: short unique identifier of a file
- **expansion**: amount of redundancy of data measured as `|stored data| / |original data|`
- **fragmentation**: amount of erasure codes output from encoding
- **store/storage node/node**: server storing and serving blocks/pieces

## Uploading File

When user wants to upload a file, space needs to be first allocated by the satellite of choice. Satellite will build the allocation structure, distributing pieces to blocks on different stores, the allocated space is always rounded up to multiple of `fragmentation * |piece| / expansion`.

```
fragmentation = 4
expansion     = 2

|piece| = 4b
|block| = 4pieces
|block-id| = 4b

|max-round-loss| = (|piece| * fragmentation / expansion) - 1  = 7b
|block-id-overhead| = |block-id| / (|piece| * |block|)        = 0.25b

|total-adressable-storage| = 2^(|block-id| * 8) * |block| * |piece| / expansion = 2^35

|file| = 235b

|allocated|   = ceil(|file| / (|piece| * fragmentation / expansion))          = 30pieces
|full-blocks| = |allocated| / (|block| / expansion)                           = 15blocks
|per-store|   = |full-blocks| / fragmentation                                 = 3.75blocks

|round-loss|     = |full-blocks| * |piece| * |block| - |file| = 5b
```

Parameter choice should minimize `|round-loss|` and `|block-id-overhead|`, but also maximize `|total-addressable-storage|`. Satellites should have a  incomplete block allocations where for small files or reminders of big files. An object in a satellite should hold `[[block; fragmentation]]`, `piece-count`, and one `in-block-start-offset` and `in-block-end-offset` to find a tail and head withing the block. A block should hold `store-id` + `block-id` to locate it.

Satellite only allocates the space on nodes so that user can directly upload to the storage nodes based of the metadata satellite sends. Metadata should be signed by the satellite so each storage node can verify users request.

## File Download

Once user needs to download a file, satellite can be contacted for the file metadata and bandwidth allocations so that the streamed download can be initiated, because of erasure coding of data, user can fetch from most local nodes in parallel an only resort to request redundant codes if decryption of a block fails. Malice can be reported to the satellite with a proof.

To prevent false reports, satellite will in addition use `Berkleamp-Welch` algorithm to identify nodes that provided incorrect pieces. Reporting nodes should be rate-limited by the satellite.

## Deletion

Its a role of each satellite to guard against unwanted deletion. User can request satellite to delete the file and notify all stores involved in storing the information. Satellite needs to manage which blocks have empty space in them for proper reuse. Data rearrangement should not be necessary since the block hash changes with deleting a file, satellite assumes store will swap remove the range of pieces. This means satellite needs to remember just how many pieces in a block are free / taken.

```
arr = [1, 2, 3, 4, 5]
arr.swap_remove 0..2
arr == [4, 5, 3]
```

## Update

In this case it depends on size difference of old and new file size. In case of truncation space is reused. In case of extension, if there is enough space in tail block, we just insert them after last piece of old file. As a last resort, tail of the file is reallocated to new block. (combined truncation and extension can be also optimized)

## Block Addressing

Allocating blocks should be paralelisable while keeping compact encoding. For this we express blocks of a file as sequence starting with random seed `S` where every block `B[i]` is computed as `hash(B[i-1])` with exception of `B[0] = S`. To better utilize blocks, the reminder block from each file is remembered by node that allocated it and used for small files that fit in. This means file does not need to remember a list of blocks but only the seed. Once allocation is made, its replicated trough the network as needed, though only the node handling the allocation is able to allocate to the block reminders which means no synchronization is needed. File can then be uniquely identified by `S + Offset + Size` which is short enough to be embedded in a link.
