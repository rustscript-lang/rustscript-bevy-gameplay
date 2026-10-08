# Xiangqi browser font

`xiangqi-cjk.otf` is a subset of Noto Sans CJK SC Regular, containing the non-ASCII characters in `examples/xiangqi.rs`.

Source: https://github.com/googlefonts/noto-cjk/blob/main/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf

License: SIL Open Font License 1.1 (see `LICENSE.txt`).

Regenerate with fontTools after changing the Chinese UI labels. Populate a `fontTools.subset.Subsetter` with all non-ASCII characters in the source, subset the upstream font, and save to `xiangqi-cjk.otf`.
