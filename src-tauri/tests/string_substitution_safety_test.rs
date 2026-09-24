// String Substitution Safety Regression Tests (Problem 12)
//
// Verifies that template substitutions and source-to-source string manipulation
// APIs never corrupt program text containing JavaScript replacement hazards:
// $, $&, $`, $', $1, </script>, </body>, template literals, etc.

#[test]
fn test_literal_dollar_replacements_preserved() {
    let template = "const API_URL = '__ENDPOINT__'; const PRICE = '__PRICE__';";
    let endpoint = "https://api.example.com/pay?$filter=active&$sort=asc";
    let price = "$100.00 (discount: $&, previous: $`, next: $')";

    let result = template
        .replace("__ENDPOINT__", endpoint)
        .replace("__PRICE__", price);

    assert!(result.contains("https://api.example.com/pay?$filter=active&$sort=asc"));
    assert!(result.contains("$100.00 (discount: $&, previous: $`, next: $')"));
}

#[test]
fn test_script_tag_and_body_inlining_safety() {
    let html_template = "<!DOCTYPE html><html><head></head><body><div id=\"root\"></div><script>__BUNDLE__</script></body></html>";
    let js_bundle = r#"
        const regex = /<\/script>/i;
        const msg = "Testing $' and $` and $& inside JS bundle";
        const cost = "$99.99";
        console.log(msg, cost);
    "#;

    let inlined = html_template.replace("__BUNDLE__", js_bundle);
    assert!(inlined.contains("const regex = /<\\/script>/i;"));
    assert!(inlined.contains("Testing $' and $` and $& inside JS bundle"));
    assert!(inlined.contains("const cost = \"$99.99\";"));
    assert!(inlined.ends_with("</script></body></html>"));
}

#[test]
fn test_template_literal_and_dollar_curly_substitution() {
    let source_template = "export const render = () => { return `__TEMPLATE_BODY__`; };";
    let complex_content = "${user.name} scored ${points} on ${date} with $& bonus";

    let substituted = source_template.replace("__TEMPLATE_BODY__", complex_content);
    assert_eq!(
        substituted,
        "export const render = () => { return `${user.name} scored ${points} on ${date} with $& bonus`; };"
    );
}
