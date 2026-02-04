# Tasty is a TOML-based API testing tool

**Tasty** is a command-line tool that runs API tests defined and grouped in TOML files.
It has an opinionated syntax for writing and storing comprehensive API tests in a folder
full of *.toml files.

## Status and Roadmap

**Expect breaking changes until `v1.0.0`.**

Tasty is being built as a replacement for bash scripts used for API testing. The v0.9.5 release introduced breaking changes to the test syntax (see Changelog), moving toward a stable v1.0.0 release.

Right now, Tasty expects that you're working with the `application/json` content type only.

## Installation

### Install with cargo

```bash
cargo install tasty
```

### Build from source

```bash
git clone https://github.com/jesselawson/tasty.git
cd tasty
cargo build --release
```

## Usage

```
$ tasty --help

Tasty, the API server testing tool

Usage: tasty [OPTIONS] [TESTS]...

Arguments:
  [TESTS]...  Specific test files to run

Options:
  -b, --base-url <BASE_URL>       Base URL for the API (defaults to http://127.0.0.1:3030)
  -t, --tests-folder <FOLDER>     Custom tests folder path
  -g, --global-timeout <SECONDS>  Global timeout in seconds [default: 30]
  -d, --debug                     Prints extra information on test run, including responses for passing tests
  -j, --json                      Output results as JSON (Not implemented yet)
  -H, --header <HEADER>           HTTP headers to include with each request (can be used multiple times)
  -h, --help                      Print help
  -V, --version                   Print version
```

### Examples

Run all *.toml test files in your current working directory's
`/api_tests` folder against `http://localhost:3030`:

```bash
tasty
```

---

Run all *.toml test files in your current working directory's
`/examples` folder against `http://localhost:3030`:

```bash
tasty -t examples
```

---

Run all *.toml test files in your current working directory's
`/examples` folder against `https://api.example.com`:

```bash
tasty -t examples http://api.example.com
```

---

Run just the `user_auth` test file in your current working
directory's `/api_tests` folder against `https://api.example.com`:

```bash
tasty -t api_tests https://api.example.com user_auth
```

---

Run the `user_signup` and `auth_flow` test files in your current working
directory's `/api_tests` folder against `http://staging-api.example.com`:

```bash
tasty http://staging-api.example.com user_signup auth_flow
```

---

Run tests with custom HTTP headers (useful for API keys or static auth tokens):

```bash
tasty -b https://api.example.com -H "Authorization: Bearer mytoken" -H "X-Api-Key: secret"
```

## Writing Tests

Tests are defined in and grouped by TOML files. If you have a
TOML file named `user_signup.toml`, all the tests inside that file
can be invoked with Tasty by passing it as a command-line argument.

Each test file can contain multiple test cases. Here's an example
of a file with a single test case:

```toml
# user_signup.toml

[accept_valid_signup]
method = "POST"
route = "auth/signup"
payload = { email = "alice@example.com", password = "SecurePassword123!" }
expect.http_status = 200
expect.response = { status = "ok" }
```

Here's an example of the same test written in a different TOML syntax:

```toml
# user_signup.toml

[accept_valid_signup]
method = "POST"
route = "auth/signup"
payload.email = "alice@example.com"
payload.password = "SecurePassword123!"
expect.http_status = 200
expect.response.status = "ok"
```

### Test File Syntax

Test files have the following properties:

* `name` _(Optional)_ The table key is the name of the test in the output report,
  but you can use this field if you'd like your table keys to be different from
  your test names.
* `method` The HTTP method to be used in the request (GET, POST, PUT, PATCH, DELETE)
* `route` The route to send the request to, not including the base URL
* `payload` A TOML table that includes the request data
* `expect.http_status` The integer HTTP response code that indicates a passing test
* `expect.response` _(Optional)_ Properties that MUST match exactly in the response (literal matching)
* `expect.response_regex` _(Optional)_ Properties that MUST match regex patterns in the response

### Regex Matching

Use `expect.response_regex` when you need to validate dynamic values:

```toml
[test_login]
method = "POST"
route = "auth/login"
payload = { username = "test_user", password = "password123" }
expect.http_status = 200
expect.response = { token_type = "Bearer" }
expect.response_regex = { access_token = "[a-zA-Z0-9_-]+" }
```

Both `expect.response` (literal) and `expect.response_regex` can be used together in the same test.

### Response Referencing

Tests can reference values from previous test responses. This is useful for authentication flows
where you need to use a token from a login response in subsequent requests:

```toml
[test_login]
method = "POST"
route = "auth/login"
payload = { username = "test_user", password = "password123" }
expect.http_status = 200
expect.response = { token_type = "Bearer" }

[test_protected_endpoint]
method = "GET"
route = "api/protected"
payload.auth_token = { from = "test_login", property = "access_token" }
expect.http_status = 200
```

The `{ from = "test_name", property = "path.to.value" }` syntax extracts values from previous
test responses using dot notation for nested access (e.g., `user.profile.id`).

If the referenced test doesn't exist or failed, the dependent test will fail with a clear error message.

## Participating & Contributing

Contributions are welcome and encouraged. Please feel free to submit a Pull Request.
For major changes, please open an issue first to discuss what you would like to change.

### Future Improvements

While `tasty` is already useful for my purposes in its current form, I am open to
enhancements that include (but are not limited to):

- optional json output of test results
- parallel test execution
- response schema validation
- custom test reporters
- environment variable substitution
- request/response logging
- response header validation


## License

This project is licensed under the GNU General Public License version 3. See
the [LICENSE](LICENSE) file for details.

## Lore

The name `tasty!` comes from the English translation of the Japanese word "umai".

As I was building an API server, I found myself digging into a particularly
complex puzzle that involved asynchronous code and mutexes. At one point I was
making changes in one window and running my tests in another window, and as each
test completed I was saying _"delicious!"_. I'm not really sure why, but stay
with me. So in the Japanese manga series _Kimetsu no Yaiba_ ("Demon Slayer"),
there's a character named Rengoku who the protagonists find eating food and saying "umai!"
after each bite. (There's a whole backstory behind why he says this after each bite of
food, which I will not get into here). So I was running these tests and reminding
myself of Rengoku as he was saying "tasty!" after each bite. And thus, _tasty_ was born.

## Changelog

### 0.9.6

* Added `-H`/`--header` flag for custom HTTP headers. Headers are applied to all requests in a test suite.

### 0.9.5 (Breaking Changes)

* **Breaking:** Replaced `expect_http_status` and `expect_response_includes` with new `expect` syntax:
  - `expect.http_status` for HTTP status code validation
  - `expect.response` for literal value matching
  - `expect.response_regex` for regex pattern matching
* Added response referencing: tests can now reference values from previous test responses using `{ from = "test_name", property = "path.to.value" }` syntax.
* Added dot-notation support for nested property access in expectations.

### 0.9.4

* Fixed an issue caused by deserializing JSON response values into a `Table` from the `toml` crate. Responses from test runs now use `serde_json::Value`.
* Enforced declaration ordering of test runs. Previously, tests were running in alphabetical order according to the table key of the defined tests. Now they will run in the order in which they appear in the test files.
* Made `url` a flag rather than a positional argument.
* The `-t` flag to provide a custom testing directory now correctly interprets relative paths.
* Continued improvements around output formatting, especially when the debug flag (`-d`/`--debug`) is passed.

### 0.9.3

* Stops testing if it can't reach the API server on the first test.

### 0.9.2

* Corrected erroneous field name in readme
* Added debug flag (`-d` or `--debug`) for the curious (and/or suspicious).
