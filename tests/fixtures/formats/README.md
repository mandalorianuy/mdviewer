# Task 9 and Task 10 format fixtures

The Task 9 `sample.*` files are byte-identical copies of the legacy Swift
fixtures. The other Task 9 fixtures are portable authored parser cases.

The Task 10 archives and images are portable, deterministic authored fixtures.
Their ZIP members use fixed stored compression, zero timestamps, UTF-8 names,
Unix regular-file modes, no platform paths, and source-order central records.
The PNG sample contains deterministic headers and semantic metadata. Local v1
accepts only non-interlaced PNG scanlines, reports that profile as
`png.interlace_profile=non_interlaced_only`, and returns typed
`UnsupportedInput` for Adam7 rather than claiming validation it does not
perform. The JPEG sample is a deterministic 1x1 image encoded with
libjpeg-turbo 3.2.0 and tagged with `wrjpgcom`; Task 10 structurally validates
compressed streams but does not render raster pixels or send either fixture to
OCR.

| Fixture | Provenance | SHA-256 |
| --- | --- | --- |
| `bounded.zip` | Task 10 authored stored ZIP | `788aae8ff3a1f48853876d132d8b2b4d24314291ce315783deb1e2831b420cb1` |
| `semantic.docx` | Task 10 authored authenticated OOXML package | `419dc2b660126552c6db381c9967549dcc53112bf2444b4991ceaf1cc306ec31` |
| `ordered.pptx` | Task 10 authored authenticated OOXML package | `34117f8143fcae8f8090bdbe1e867240225170cffe29ad137db4a2dd05016cb7` |
| `displayed.xlsx` | Task 10 authored authenticated OOXML package | `f6fcf0b389320a861993e9207905886429349b6bf349553e2d67a0dad5a5aa77` |
| `spine.epub` | Task 10 authored namespace-canonical EPUB package | `97e4b332f5a7013f5b4ff3bc97501a932c6624ff1bc952f3e5414111343e097e` |
| `metadata.png` | Task 10 authored structurally valid PNG metadata fixture | `71dc4d61468db9840d56618e134e80e5d0ab3754668b8ec19e479f29df516fab` |
| `metadata.jpg` | Task 10 libjpeg-turbo 3.2.0 JPEG plus deterministic comment | `14f7a5e76f7cb0210ae140c4fbcd2ad77a9605bdc281a0557fd00dbb6024ad31` |

Corresponding `tests/golden/formats/*.md` files are emitted only through
`mdconvert_core::emit_gfm`; converters do not construct Markdown strings.
