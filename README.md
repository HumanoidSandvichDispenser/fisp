# Fisp

**FI**le**S**ystem **P**rocessing: a FUSE Lisp esolang because
everything is a file!

This Lisp implements the `atom`, `eq`, `car`, `cdr`, `cons` primitive
functions, the `+`, `-`, `mul` (`*`), `div` (`/`) arithmetic functions, as well
as the `if`, `lambda`, `call`, and `quote` special forms.

`*` and `/` are spelled `mul` and `div` in the filesystem, respectively, to
avoid shell globbing and path name restrictions.

## Syntax

The syntax of Fisp is a Polish notation without parentheses. As a result,
variadic functions can be defined, but they must be sequenced with `call`
calls. For example, the following expression:

```lisp
+/1/2/3
```

will fail because `+` is a function that expects 2 arguments.

### Currying

Function definitions must be explicitly curried. For example, the following expression:

```lisp
(lambda (x y) (+ x y))
=> (lambda (x) (lambda (y) (+ x y)))
```

must be written as:

```bash
lambda/x/lambda/y/+/x/y
```

## Example

This program calculates the factorial of a number:

```bash
$ ln -s lambda/n/if/=/n/0/1/*/n/factorial/-/n/1 factorial
$ cat factorial/5
120
```

This program calculates the Fibonacci number of a number:

```bash
$ ln -s lambda/n/if/=/n/0/0/if/=/n/1/1/fibonacci/-/n/1/fibonacci/-/n/2/+/*/n/fibonacci/-/n/1/fibonacci fibonacci
$ cat fibonacci/10
55
```
