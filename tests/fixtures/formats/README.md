# Task 9 and Task 10 format fixtures

The Task 9 `sample.*` files are byte-identical copies of the legacy Swift
fixtures. The other Task 9 fixtures are portable authored parser cases.

The Task 10 archives and images are portable, deterministic authored fixtures.
Their ZIP members use fixed stored compression, zero timestamps, UTF-8 names,
Unix regular-file modes, no platform paths, and source-order central records.
The PNG/JPEG samples contain only deterministic headers and semantic metadata;
their pixels are never decoded or sent to OCR.

| Fixture | Provenance | SHA-256 |
| --- | --- | --- |
| `bounded.zip` | Task 10 authored stored ZIP | `69cd980ef996f02fd33f59f2cb9e91ba5df72d0265076a96edfa1d795a53bcfd` |
| `semantic.docx` | Task 10 authored minimal OOXML package | `0f45feccbf38c601c691b9cbac779b8a93f425734b9eafccf13b312093d675aa` |
| `ordered.pptx` | Task 10 authored minimal OOXML package | `504277327962f2b45dc20d3b2776f120a49b50903f29115da9704cdefff0fd6f` |
| `displayed.xlsx` | Task 10 authored minimal OOXML package | `398c7261e89d86bd73d987cdfb66d2aa15dbbcd7009e64274b68d766924bf396` |
| `spine.epub` | Task 10 authored EPUB package | `515b5b8c96399c79f79226b19886a6bfd6a1eb3b8bcc95bdbec94592669c3b9b` |
| `metadata.png` | Task 10 authored PNG metadata fixture | `f04e818d291f20fbe6dec8ec1ad52452a7c810a9caa944d35478b57ba40bfbc9` |
| `metadata.jpg` | Task 10 authored JPEG metadata fixture | `82c99cea987ee571b2c0a6bd538a302dac359bf44e6eb9baa40d7748091d2547` |

Corresponding `tests/golden/formats/*.md` files are emitted only through
`mdconvert_core::emit_gfm`; converters do not construct Markdown strings.
