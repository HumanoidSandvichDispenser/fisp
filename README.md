# Fisp

**FI**le**S**ystem **P**rocessing: a FUSE Lisp esolang because
everything is a file!

This Lisp implements:

- `atom`, `eq`, `car`, `cdr`, `cons` primitive functions
- the `+`, `-`, `mul`/`*`, `div`, `mod` arithmetic functions
- the `=`, `<`, `>` comparison functions
- `if`, `lambda`/`λ`, `call`/`@`, `eval`, and `quote`/`^` special forms.
- `show` debugging function
- `implode`, `explode` string functions

## Syntax

The syntax of Fisp is strict Polish notation (without parentheses). As a
result, arity of user-defined function calls must be explicitly specified using
the application (`call` or `@`) operator. For built-in functions, these will
have known constant arity at parse time, and will not be required to have their
arity specified.

```sh
cat @@my-function/arg1/arg2
```

### Currying

Closures must be explicitly curried. For example, the following expression:

```lisp
(lambda (x y) (+ x y))
;; => (lambda (x) (lambda (y) (+ x y)))
```

must be written as:

```sh
lambda/x/lambda/y/+/x/y
```

### Global definitions

Global/top-level path-expressions can be created by creating a symbolic link to
the path-expression.

```bash
ln -s 2 my-number
cat my-number
# => 2
ln -s lambda/x/+/x/1 add-one
cat @add-one/6
# => 7
ln -s lambda/x/+/my-number/x add-my-number
cat @add-my-number/7
# => 9
```

## Example Programs

This program (naively) calculates the factorial of a number:

```sh
ln -s lambda/n/if/=/n/0/1/mul/n/@factorial/-/n/1 factorial
cat factorial/5
# => 120
```

This program (tail-recursively) calculates the nth Fibonacci number:

```sh
ln -s 'λ/n/λ/a/λ/b/if/=/n/0/a/@@@fibi/-/n/1/b/+/a/b' fibi
ln -s 'λ/n/@@@fibi/n/0/1' fastfib
cat fastfib/20
# => 6765
```

This program implements `append` and `string-append` functions:

```sh
ln -s lambda/list1/lambda/list2/if/eq/list1/nil/list2/cons/car/list1/@@append/cdr/list1/list2 append
ln -s lambda/str1/lambda/str2/implode/@@append/explode/str1/explode/str2 string-append
cat @@string-append/:hello%20/:world
# => "hello world"
```
