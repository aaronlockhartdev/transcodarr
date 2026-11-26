# Editing files

Edit files with the anchored tools (`replace`, `replace_match`, `insert`, `copy`, `move`), never by
running `python3`/`sed`/`perl` through `bash`: a script that rewrites a file exits 0 even when it
matched nothing or dropped a line, and no gate can see that afterwards. Land computed content with
`write`. `/tmp` scratch is exempt.
