
# macOS Integration

## Filesystem

Do not shell out to `ls`.

Use Rust filesystem APIs directly.

## Spotlight metadata

Initial attributes:

* kMDItemContentType
* kMDItemKind
* kMDItemTitle
* kMDItemAuthors
* kMDItemKeywords
* kMDItemWhereFroms
* kMDItemFSCreationDate
* kMDItemFSContentChangeDate

## Spotlight Search

Spotlight is used as a candidate discovery mechanism.

It should not be treated as the authoritative filesystem state.

Authoritative state comes from the filesystem.

## Extended Attributes

Extended attributes provide additional provenance and
macOS-specific metadata.

## Future: FSEvents

Filesystem events will allow incremental processing instead
of repeatedly scanning complete directories.
