# tasty! 🍰

**Tasty** is a command-line tool that runs API tests defined and grouped in TOML files.

## Status and Roadmap

**Expect this project to be updated at least once or twice per month until `v1.0.0`.**

* Tasty is being built as a replacement for my bash scripts that I use for API testing. As I migrate features into Tasty, I'll release updates to this project. 
* All releases will aim to be backwards-compatible. That includes keeping the way the testing files are written (e.g., using table keys as test names). 

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

Usage: tasty [OPTIONS] [URL] [TESTS]...

Arguments:
  [URL]       Base URL for the API (defaults to http://127.0.0.1:3030)
  [TESTS]...  Specific test files to run

Options:
  -t, --tests-folder <FOLDER>     Custom tests folder path
  -g, --global-timeout <SECONDS>  Global timeout in seconds [default: 30]
  -j, --json                      Output results as JSON (Not implemented yet)
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

## Writing Tests

Tests are defined in and grouped by TOML files. If you have a 
TOML file named `user_signup.toml`, all the tests in side that file 
can be invoked with Tasty by passing it as a command-line argument. 

Each test file can contain multiple test cases. Here's an example
of a file with a single test case:

```toml
# user_signup.toml

[accept_valid_signup]
method = "POST"
route = "auth/signup"
payload = { email = "alice@example.com", password = "This is a Valid Password!@t%" }
expect_http_status = 200
expect_response_includes = { status = "ok" }
```

Here's an example of the same test written in a different TOML syntax: 

```toml
# user_signup.toml

[accept_valid_signup]
method = "POST"
route = "auth/signup"
payload.email = "alice@example.com"
payload.password = "This is a Valid Password!@t%"
expect_http_status = 200
expect_response_includes.status = "ok"
```

### Test File Syntax

Test files have the following properties that MUST be present 
in each table:

* `name` _(Optional)_ The table key is the name of the test in the output report, 
  but you can use this field if you'd like your table keys to be different from 
  your test names. You might want this if you prefer the table keys in your TOML 
  files to be organized differently than by the name of each test.
* `method` The HTTP method to be used in the request
* `route` The route to send the request to, not including the base URL
* `payload` A TOML table that includes the request data
* `expect_http_status` The integer HTTP response code that indicates a passing test
* `expect_response_includes` _(Optional)_ One or more properties that MUST be present in the response payload

## Participating & Contributing

Contributions are welcome and encouraged. Please feel free to submit a Pull Request. 
For major changes, please open an issue first to discuss what you would like to change.

### Future Improvements

While `tasty` is already useful for my purposes in its current form, I am open to 
backwards-compatible enhancements that include (but are not limited to):

- passing response values (like a a JWT) from one test to another
- optional json output of test results
- parallel test execution
- test dependencies and ordering
- response schema validation
- custom test reporters
- environment variable substitution
- request/response logging


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

### 0.9.5

* Fixed an issue caused by deserializing JSON response values into a `Table` from the `toml` crate. Responses from test runs now use `serde_json::Value`.
* Enforced declaration ordering of test runs. Previously, tests were running in alphabetical order according to the table key of the defined tests. Now they will run in the order in which they appear in the test files. 
* Made `url` a flag rather than a positional argument.

### 0.9.4

* The `-t` flag to provide a custom testing directory now correctly interprets relative paths. Before, passing `-t example` would not read from the `example` folder in the current working directory. Now, you can either specify a relative path or a full path. 
* Continued improvements around output formatting, especially when the debug flag (`d`/`--debug`) is passed.

### 0.9.3

* Stops testing if it can't reach the API server on the first test.

### 0.9.2

* Corrected erroneous field name in readme
* Added debug flag (`-d` or `--debug`) for the curious (and/or suspicious). 
