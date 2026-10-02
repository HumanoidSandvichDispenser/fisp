# Lisf

A filesystem-based Lisp esolang because everything is a file.

This Lisp implements the `atom`, `eq`, `car`, `cdr`, `cons` primitive
functions, the `+`, `-`, `mul` (`*`), `div` (`/`) arithmetic functions, as well
as the `if`, `lambda`, and `quote` special forms.

`*` and `/` are spelled `mul` and `div` in the filesystem, respectively, to
avoid shell globbing and path name restrictions.

## Example

This program calculates the factorial of a number:

```bash
$ ln -s lambda/n/if/=/n/0/1/*/n/factorial/-/n/1 factorial
$ cat factorial/5
120
```
