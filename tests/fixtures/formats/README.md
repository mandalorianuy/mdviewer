# Task 9 and Task 10 format fixtures

The Task 9 `sample.*` files are byte-identical copies of the legacy Swift
fixtures. The other Task 9 fixtures are portable authored parser cases.

The Task 10 archives and images are portable, deterministic authored fixtures.
Their ZIP members use fixed stored compression, zero timestamps, UTF-8 names,
Unix regular-file modes, no platform paths, and source-order central records.
The PNG sample contains deterministic headers and semantic metadata. The JPEG
sample is a deterministic 1x1 image encoded with libjpeg-turbo 3.2.0 and tagged
with `wrjpgcom`; neither fixture is decoded to pixels or sent to OCR by Task 10.

| Fixture | Provenance | SHA-256 |
| --- | --- | --- |
| `bounded.zip` | Task 10 authored stored ZIP | `788aae8ff3a1f48853876d132d8b2b4d24314291ce315783deb1e2831b420cb1` |
| `semantic.docx` | Task 10 authored authenticated OOXML package | `9efd96a953f356ee200b5cefdfe6c2c0f78003817697579594b2ef9fab25810d` |
| `ordered.pptx` | Task 10 authored authenticated OOXML package | `34117f8143fcae8f8090bdbe1e867240225170cffe29ad137db4a2dd05016cb7` |
| `displayed.xlsx` | Task 10 authored authenticated OOXML package | `f6fcf0b389320a861993e9207905886429349b6bf349553e2d67a0dad5a5aa77` |
| `spine.epub` | Task 10 authored namespace-canonical EPUB package | `a81fd9e379a9673cb00ce5ab18f6f6554b369010e0c1e1aa7f1cacaa861cea09` |
| `metadata.png` | Task 10 authored PNG metadata fixture | `f04e818d291f20fbe6dec8ec1ad52452a7c810a9caa944d35478b57ba40bfbc9` |
| `metadata.jpg` | Task 10 libjpeg-turbo 3.2.0 JPEG plus deterministic comment | `14f7a5e76f7cb0210ae140c4fbcd2ad77a9605bdc281a0557fd00dbb6024ad31` |

Corresponding `tests/golden/formats/*.md` files are emitted only through
`mdconvert_core::emit_gfm`; converters do not construct Markdown strings.
