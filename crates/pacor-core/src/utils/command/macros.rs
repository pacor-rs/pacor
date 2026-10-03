/// Constructs a `Vec<OsString>` from a list of expressions that implement [`ToOsString`].
///
/// This macro simplifies building command-line argument lists by automatically converting
/// common types like `&str`, `Path`, and any type implementing `Display` into `OsString`.
/// It supports multiple syntax forms:
/// - Basic form: `command_args!(arg1, arg2, ...)` - creates a vector from individual arguments
/// - Slice expansion: `command_args!(&slice; ..)` - creates a vector from a slice
/// - Pre-allocated form: `command_args!(capacity = N, arg1, arg2, ...)` - creates a vector with specified capacity
/// - Pre-allocated slice: `command_args!(capacity = N, &slice; ..)` - creates a vector with capacity from a slice
///
/// # Arguments
///
/// * `capacity` - Optional. Specifies the initial capacity for the vector to avoid reallocations
/// * `arg` - One or more expressions implementing [`ToOsString`]. Supports trailing commas.
/// * `slice; ..` - A slice of arguments to expand, using `; ..` syntax to indicate expansion
///
/// # Examples
///
/// Basic usage with individual arguments:
/// ```rust
/// # use std::path::Path;
/// # use crate::utils::command::macros::command_args;
///
/// let args = command_args!("hello", Path::new("/tmp"), 42u32);
/// assert_eq!(args.len(), 3);
/// assert_eq!(args[0].to_str(), Some("hello"));
/// assert_eq!(args[1].to_str(), Some("/tmp"));
/// assert_eq!(args[2].to_str(), Some("42"));
/// ```
///
/// Expanding a slice:
/// ```rust
/// # use crate::utils::command::macros::command_args;
///
/// let slice = ["foo", "bar", "baz"];
/// let args = command_args!(&slice; ..);
/// assert_eq!(args.len(), 3);
/// assert_eq!(args[0].to_str(), Some("foo"));
/// assert_eq!(args[1].to_str(), Some("bar"));
/// ```
///
/// With pre-allocated capacity:
/// ```rust
/// # use std::path::Path;
/// # use crate::utils::command::macros::command_args;
///
/// let args = command_args!(capacity = 10, "arg1", "arg2", "arg3");
/// assert_eq!(args.len(), 3);
/// assert_eq!(args.capacity(), 10);
/// ```
///
/// Expanding a slice with pre-allocated capacity:
/// ```rust
/// # use crate::utils::command::macros::command_args;
///
/// let slice = ["foo", "bar"];
/// let args = command_args!(capacity = 10, &slice; ..);
/// assert_eq!(args.len(), 2);
/// assert_eq!(args.capacity(), 10);
/// ```
///
/// With trailing comma:
/// ```rust
/// # use crate::utils::command::macros::command_args;
///
/// let args = command_args!("a", "b", "c",); // Note the trailing comma
/// assert_eq!(args.len(), 3);
/// ```
#[macro_export]
macro_rules! command_args {
    // Handle slice expansion: use `; ..` syntax to explicitly indicate expansion
    ($slice:expr; ..) => {{
        use $crate::utils::command::ToOsString;
        $slice.iter().map(|arg| arg.to_os_string()).collect::<Vec<_>>()
    }};

    // Handle slice expansion with capacity
    (capacity = $cap:expr, $slice:expr; ..) => {{
        use $crate::utils::command::ToOsString;
        let mut vec = Vec::with_capacity($cap);
        vec.extend($slice.iter().map(|arg| arg.to_os_string()));
        vec
    }};

    // Original multi-argument form with capacity
    (capacity = $cap:expr, $($arg:expr),+ $(,)?) => {{
        use $crate::utils::command::ToOsString;
        let mut vec = Vec::with_capacity($cap);
        $(vec.push($arg.to_os_string());)*
        vec
    }};

    // Original multi-argument form
    ($($arg:expr),+ $(,)?) => {{
        use $crate::utils::command::ToOsString;
        vec![$($arg.to_os_string()),*]
    }};
}

/// Appends a single argument (converted via [`ToOsString`]) to an existing `Vec<OsString>`.
///
/// This is useful when building command arguments incrementally.
///
/// # Examples
///
/// ```rust
/// # use std::path::Path;
/// # use crate::utils::command::macros::{command_args, command_args_push};
///
/// let mut args = command_args!("base");
/// command_args_push!(args, "--flag");
/// command_args_push!(args, Path::new("/output"));
///
/// assert_eq!(args.len(), 3);
/// assert_eq!(args[1].to_str(), Some("--flag"));
/// ```
#[macro_export]
macro_rules! command_args_push {
    ($vec:expr, $arg:expr) => {{
        use $crate::utils::command::ToOsString;
        $vec.push($arg.to_os_string());
    }};
}

/// Extends a `Vec<OsString>` with multiple arguments, each converted via [`ToOsString`].
///
/// This macro provides a convenient way to add several arguments at once to an existing
/// argument vector. It supports two patterns:
/// - Extending with a slice of arguments
/// - Extending with multiple individual arguments
///
/// # Patterns
///
/// ## Slice Extension
/// Extends the vector with all elements from a slice or iterator:
/// ```rust
/// # use crate::utils::command::macros::command_args_extend;
/// # let mut args = vec![];
/// let extra_args = ["--verbose", "test"];
/// command_args_extend!(args, &extra_args);
/// ```
///
/// ## Individual Arguments
/// Extends the vector with multiple individual arguments:
/// ```rust
/// # use crate::utils::command::macros::command_args_extend;
/// # let mut args = vec![];
/// command_args_extend!(args, "--verbose", 42, "test");
/// ```
///
/// # Examples
///
/// ```rust
/// # use crate::utils::command::macros::{command_args, command_args_extend};
///
/// // Using individual arguments
/// let mut args = command_args!("cmd");
/// command_args_extend!(args, "--verbose", 42, "test");
///
/// assert_eq!(args.len(), 4);
/// assert_eq!(args[3].to_str(), Some("test"));
///
/// // Using a slice
/// let mut args = command_args!("cmd");
/// let extra_args = ["--debug", "file.txt"];
/// command_args_extend!(args, &extra_args);
///
/// assert_eq!(args.len(), 3);
/// assert_eq!(args[2].to_str(), Some("file.txt"));
/// ```
#[macro_export]
macro_rules! command_args_extend {
    ($vec:expr, $slice:expr) => {{
        use $crate::utils::command::ToOsString;
        $vec.extend($slice.iter().map(|arg| arg.to_os_string()));
    }};
    ($vec:expr, $first:expr, $second:expr $(, $rest:expr)* $(,)?) => {{
        use $crate::utils::command::ToOsString;
        $vec.extend([$first.to_os_string(), $second.to_os_string() $(, $rest.to_os_string())*]);
    }};
}

// Re-exported here so call sites can keep using the `utils::command::macros`
// path even though `#[macro_export]` places the macros at the crate root.
pub use crate::{command_args, command_args_extend, command_args_push};

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::Path};

    #[test]
    fn test_command_args_basic() {
        let args = command_args!("hello", "world");
        assert_eq!(args, vec![OsString::from("hello"), OsString::from("world")]);
    }

    #[test]
    fn test_command_args_single_argument() {
        let args = command_args!("single");
        assert_eq!(args, vec![OsString::from("single")]);
    }

    #[test]
    fn test_command_args_with_path_and_display() {
        let args = command_args!(
            "build",
            Path::new("/tmp/output"),
            42u32,
            true,
            "done".to_string(),
            0.881
        );
        assert_eq!(args.len(), 6);
        assert_eq!(args[0].to_str(), Some("build"));
        assert_eq!(args[1].to_str(), Some("/tmp/output"));
        assert_eq!(args[2].to_str(), Some("42"));
        assert_eq!(args[3].to_str(), Some("true"));
        assert_eq!(args[4].to_str(), Some("done"));
        assert_eq!(args[5].to_str(), Some("0.881"));
    }

    #[test]
    fn test_command_args_trailing_comma() {
        let args = command_args!("a", "b",);
        assert_eq!(args.len(), 2);
        assert_eq!(args, vec![OsString::from("a"), OsString::from("b")]);
    }

    #[test]
    fn test_command_args_with_capacity() {
        let args = command_args!(capacity = 10, "arg1", "arg2", "arg3");
        assert_eq!(args.len(), 3);
        assert_eq!(args.capacity(), 10);
        assert_eq!(
            args,
            vec![
                OsString::from("arg1"),
                OsString::from("arg2"),
                OsString::from("arg3")
            ]
        );
    }

    #[test]
    fn test_command_args_with_capacity_and_trailing_comma() {
        let args = command_args!(capacity = 5, "x", "y", "z",);
        assert_eq!(args.len(), 3);
        assert_eq!(args.capacity(), 5);
    }

    #[test]
    fn test_command_args_mixed_types_with_capacity() {
        let args = command_args!(capacity = 20, "str", Path::new("path"), 123, true);
        assert_eq!(args.len(), 4);
        assert_eq!(args.capacity(), 20);
    }

    #[test]
    fn test_command_args_slice_expansion() {
        let slice = ["foo", "bar", "baz"];
        let args = command_args!(&slice; ..);

        assert_eq!(args.len(), 3);
        assert_eq!(args[0].to_str(), Some("foo"));
        assert_eq!(args[1].to_str(), Some("bar"));
        assert_eq!(args[2].to_str(), Some("baz"));
    }

    #[test]
    fn test_command_args_slice_expansion_empty() {
        let slice: &[&str] = &[];
        let args = command_args!(&slice; ..);

        assert_eq!(args.len(), 0);
    }

    #[test]
    fn test_command_args_slice_expansion_single_element() {
        let slice = ["single"];
        let args = command_args!(&slice; ..);

        assert_eq!(args.len(), 1);
        assert_eq!(args[0].to_str(), Some("single"));
    }

    #[test]
    fn test_command_args_slice_expansion_with_capacity() {
        let slice = ["foo", "bar"];
        let args = command_args!(capacity = 10, &slice; ..);

        assert_eq!(args.len(), 2);
        assert_eq!(args.capacity(), 10);
        assert_eq!(args[0].to_str(), Some("foo"));
        assert_eq!(args[1].to_str(), Some("bar"));
    }

    #[test]
    fn test_command_args_slice_expansion_with_exact_capacity() {
        let slice = ["a", "b", "c"];
        let args = command_args!(capacity = 3, &slice; ..);

        assert_eq!(args.len(), 3);
        assert_eq!(args.capacity(), 3);
    }

    #[test]
    fn test_command_args_slice_expansion_with_smaller_capacity() {
        let slice = ["a", "b", "c", "d", "e"];
        let args = command_args!(capacity = 2, &slice; ..);

        assert_eq!(args.len(), 5);
        // Capacity will be at least 5 due to reallocation
        assert!(args.capacity() >= 5);
    }

    #[test]
    fn test_command_args_slice_expansion_with_paths() {
        let paths = [Path::new("/tmp/file1.txt"), Path::new("/tmp/file2.txt")];
        let args = command_args!(&paths; ..);

        assert_eq!(args.len(), 2);
        assert_eq!(args[0].to_str(), Some("/tmp/file1.txt"));
        assert_eq!(args[1].to_str(), Some("/tmp/file2.txt"));
    }

    #[test]
    fn test_command_args_slice_expansion_with_numbers() {
        let numbers = [42u32, 123u32, 456u32];
        let args = command_args!(&numbers; ..);

        assert_eq!(args.len(), 3);
        assert_eq!(args[0].to_str(), Some("42"));
        assert_eq!(args[1].to_str(), Some("123"));
        assert_eq!(args[2].to_str(), Some("456"));
    }

    #[test]
    fn test_command_args_slice_expansion_with_string_slice() {
        let strings = [String::from("hello"), String::from("world")];
        let args = command_args!(&strings; ..);

        assert_eq!(args.len(), 2);
        assert_eq!(args[0].to_str(), Some("hello"));
        assert_eq!(args[1].to_str(), Some("world"));
    }

    // #[test]
    // fn test_command_args_slice_expansion_with_osstring_slice() {
    //     let osstrings = [OsString::from("arg1"), OsString::from("arg2")];
    //     let args = command_args!(&osstrings; ..);

    //     assert_eq!(args.len(), 2);
    //     assert_eq!(args[0].to_str(), Some("arg1"));
    //     assert_eq!(args[1].to_str(), Some("arg2"));
    // }

    #[test]
    fn test_command_args_slice_expansion_mixed_types() {
        // Test with a slice of different types that implement ToOsString
        let str_slice = ["--flag", "value"];
        let args1 = command_args!(&str_slice; ..);

        let path_slice = [Path::new("/tmp")];
        let args2 = command_args!(&path_slice; ..);

        assert_eq!(args1.len(), 2);
        assert_eq!(args2.len(), 1);
    }

    #[test]
    fn test_command_args_vec_expansion() {
        // Test with Vec instead of array
        let vec = vec!["foo", "bar", "baz"];
        let args = command_args!(&vec; ..);

        assert_eq!(args.len(), 3);
        assert_eq!(args[0].to_str(), Some("foo"));
    }

    #[test]
    fn test_command_args_slice_expansion_with_unicode() {
        let slice = ["你好", "世界", "🦀"];
        let args = command_args!(&slice; ..);

        assert_eq!(args.len(), 3);
        assert_eq!(args[0].to_str(), Some("你好"));
        assert_eq!(args[1].to_str(), Some("世界"));
        assert_eq!(args[2].to_str(), Some("🦀"));
    }

    // Regression tests: ensure old functionality still works
    #[test]
    fn test_command_args_individual_args_still_works() {
        let args = command_args!("hello", "world");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0].to_str(), Some("hello"));
        assert_eq!(args[1].to_str(), Some("world"));
    }

    #[test]
    fn test_command_args_with_capacity_still_works() {
        let args = command_args!(capacity = 10, "arg1", "arg2");
        assert_eq!(args.len(), 2);
        assert_eq!(args.capacity(), 10);
    }

    #[test]
    fn test_command_args_push_basic() {
        let mut args = command_args!("base");
        command_args_push!(args, "--flag");
        command_args_push!(args, Path::new("/out"));
        command_args_push!(args, 42);

        assert_eq!(args.len(), 4);
        assert_eq!(args[0].to_str(), Some("base"));
        assert_eq!(args[1].to_str(), Some("--flag"));
        assert_eq!(args[2].to_str(), Some("/out"));
        assert_eq!(args[3].to_str(), Some("42"));
    }

    #[test]
    fn test_command_args_push_on_empty_vec() {
        let mut args: Vec<OsString> = Vec::new();
        command_args_push!(args, "first");
        command_args_push!(args, "second");

        assert_eq!(args.len(), 2);
        assert_eq!(args[0].to_str(), Some("first"));
        assert_eq!(args[1].to_str(), Some("second"));
    }

    #[test]
    fn test_command_args_push_multiple_types() {
        let mut args = Vec::new();
        command_args_push!(args, "string");
        command_args_push!(args, Path::new("/tmp"));
        command_args_push!(args, 3.15f64);
        command_args_push!(args, false);

        assert_eq!(args.len(), 4);
        assert_eq!(args[0].to_str(), Some("string"));
        assert_eq!(args[1].to_str(), Some("/tmp"));
        assert_eq!(args[2].to_str(), Some("3.15"));
        assert_eq!(args[3].to_str(), Some("false"));
    }

    #[test]
    fn test_command_args_extend_individual_args() {
        let mut args = command_args!("cmd");
        command_args_extend!(args, "--verbose", 3, "test");

        assert_eq!(args.len(), 4);
        assert_eq!(args[0].to_str(), Some("cmd"));
        assert_eq!(args[1].to_str(), Some("--verbose"));
        assert_eq!(args[2].to_str(), Some("3"));
        assert_eq!(args[3].to_str(), Some("test"));
    }

    #[test]
    fn test_command_args_extend_individual_args_with_trailing_comma() {
        let mut args = command_args!("start");
        command_args_extend!(args, "a", "b", "c",);

        assert_eq!(args.len(), 4);
        assert_eq!(args[3].to_str(), Some("c"));
    }

    #[test]
    fn test_command_args_extend_with_slice() {
        let mut args = command_args!("base");
        let extra_args = ["--verbose", "test", "file.txt"];
        command_args_extend!(args, &extra_args);

        assert_eq!(args.len(), 4);
        assert_eq!(args[1].to_str(), Some("--verbose"));
        assert_eq!(args[2].to_str(), Some("test"));
        assert_eq!(args[3].to_str(), Some("file.txt"));
    }

    #[test]
    fn test_command_args_extend_with_vec_slice() {
        let mut args = command_args!("cmd");
        let extra_args = vec!["arg1", "arg2", "arg3"];
        command_args_extend!(args, &extra_args);

        assert_eq!(args.len(), 4);
        assert_eq!(args[3].to_str(), Some("arg3"));
    }

    #[test]
    fn test_command_args_extend_with_empty_slice() {
        let mut args = command_args!("base");
        let empty_args: [&str; 0] = [];
        command_args_extend!(args, &empty_args);

        // Should remain unchanged
        assert_eq!(args.len(), 1);
        assert_eq!(args[0].to_str(), Some("base"));
    }

    #[test]
    fn test_command_args_extend_on_empty_vec() {
        let mut args: Vec<OsString> = Vec::new();
        command_args_extend!(args, "first", "second");

        assert_eq!(args.len(), 2);
        assert_eq!(args[0].to_str(), Some("first"));
        assert_eq!(args[1].to_str(), Some("second"));
    }

    #[test]
    fn test_command_args_extend_mixed_types() {
        let mut args = command_args!("start");
        command_args_extend!(args, Path::new("/path"), 42, "end");

        assert_eq!(args.len(), 4);
        assert_eq!(args[1].to_str(), Some("/path"));
        assert_eq!(args[2].to_str(), Some("42"));
        assert_eq!(args[3].to_str(), Some("end"));
    }

    #[test]
    fn test_command_args_extend_only_two_args() {
        let mut args = command_args!("base");
        command_args_extend!(args, "one", "two");

        assert_eq!(args.len(), 3);
        assert_eq!(args[1].to_str(), Some("one"));
        assert_eq!(args[2].to_str(), Some("two"));
    }

    #[test]
    fn test_integration_all_macros_together() {
        // Start with command_args
        let mut args = command_args!(capacity = 10, "program", "--init");

        // Use push to add single arguments
        command_args_push!(args, "-v");
        command_args_push!(args, Path::new("/input"));

        // Use extend to add multiple arguments
        command_args_extend!(args, "--output", "/tmp", "file.txt");

        // Use extend with slice
        let more_args = ["--final", "end"];
        command_args_extend!(args, &more_args);

        assert_eq!(args.len(), 9);
        assert_eq!(args.capacity(), 10);

        // Verify all arguments
        let expected_args = [
            "program", "--init", "-v", "/input", "--output", "/tmp", "file.txt", "--final", "end",
        ];
        for (i, expected) in expected_args.iter().enumerate() {
            assert_eq!(args[i].to_str(), Some(*expected));
        }
    }

    #[test]
    fn test_command_args_with_various_numeric_types() {
        let args = command_args!(
            42u8,
            1000u16,
            100000u32,
            10000000000u64,
            -42i8,
            -1000i16,
            -100000i32,
            -10000000000i64,
            3.15f32,
            2.818f64
        );

        assert_eq!(args.len(), 10);
        assert_eq!(args[0].to_str(), Some("42"));
        assert_eq!(args[1].to_str(), Some("1000"));
        assert_eq!(args[2].to_str(), Some("100000"));
        assert_eq!(args[3].to_str(), Some("10000000000"));
        assert_eq!(args[4].to_str(), Some("-42"));
        assert_eq!(args[5].to_str(), Some("-1000"));
        assert_eq!(args[6].to_str(), Some("-100000"));
        assert_eq!(args[7].to_str(), Some("-10000000000"));
        assert_eq!(args[8].to_str(), Some("3.15"));
        assert_eq!(args[9].to_str(), Some("2.818"));
    }
}
