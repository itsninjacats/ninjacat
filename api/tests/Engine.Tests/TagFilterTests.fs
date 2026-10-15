module NinjaCat.Api.Engine.Tests.TagFilterTests

open Xunit
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.MetricQuery.Parser

let private parses (input: string) (expected: TagFilter) =
    match parseTagFilter input with
    | Ok actual -> Assert.Equal(expected, actual)
    | Error e -> Assert.Fail $"'{input}' did not parse:\n{e}"

let private fails (input: string) (fragment: string) =
    match parseTagFilter input with
    | Ok actual -> Assert.Fail $"'{input}' should not parse, got {actual}"
    | Error e -> Assert.Contains(fragment, e)

// --- everything -----------------------------------------------------------

[<Fact>]
let ``star and empty both mean all`` () =
    parses "{*}" All
    parses "{}" All
    parses "{ * }" All

// --- symbolic syntax ------------------------------------------------------

[<Fact>]
let ``single tag`` () = parses "{env:prod}" (Tag("env", "prod"))

[<Fact>]
let ``comma means AND`` () =
    parses "{environment:foo,host:foo}" (And [ Tag("environment", "foo"); Tag("host", "foo") ])

[<Fact>]
let ``bang negates`` () =
    // Seen in datadog-api-client-go SLO cassettes.
    parses "{type:good,!type:ignored}" (And [ Tag("type", "good"); Not(Tag("type", "ignored")) ])

[<Fact>]
let ``key-less tags, alone and mixed`` () =
    // terraform-provider-datadog TestAccDatadogMonitor_Updated.
    parses "{example,framework:chronos}" (And [ Bare "example"; Tag("framework", "chronos") ])

[<Fact>]
let ``wildcards are kept verbatim`` () =
    parses "{!device:/dev/loop*}" (Not(Tag("device", "/dev/loop*")))
    parses "{region:*east*}" (Tag("region", "*east*"))

[<Fact>]
let ``value may contain further colons`` () =
    parses "{url:http://x}" (Tag("url", "http://x"))

[<Fact>]
let ``space after the colon`` () =
    // terraform-provider-datadog resource_datadog_monitor_test.go.
    parses "{servicename: ec2}" (Tag("servicename", "ec2"))

[<Fact>]
let ``template variables`` () =
    parses "{$troux_uuid,$container_guid}" (And [ Bare "$troux_uuid"; Bare "$container_guid" ])

[<Fact>]
let ``spaces around commas`` () =
    parses "{ a:1 , b:2 }" (And [ Tag("a", "1"); Tag("b", "2") ])

// --- functional syntax ----------------------------------------------------

[<Fact>]
let ``AND and OR with parentheses`` () =
    // Datadog docs, advanced filtering.
    parses
        "{env:staging AND (availability-zone:us-east-1a OR availability-zone:us-east-1c)}"
        (And [ Tag("env", "staging")
               Or [ Tag("availability-zone", "us-east-1a"); Tag("availability-zone", "us-east-1c") ] ])

[<Fact>]
let ``AND binds tighter than OR`` () =
    parses "{a:1 OR b:2 AND c:3}" (Or [ Tag("a", "1"); And [ Tag("b", "2"); Tag("c", "3") ] ])

[<Fact>]
let ``IN and NOT IN`` () =
    parses
        "{env:shop.ist AND availability-zone IN (us-east-1a, us-east-1b, us-east4-b)}"
        (And [ Tag("env", "shop.ist"); In("availability-zone", [ "us-east-1a"; "us-east-1b"; "us-east4-b" ]) ])

    parses
        "{env:prod AND location NOT IN (atlanta,seattle,las-vegas)}"
        (And [ Tag("env", "prod"); Not(In("location", [ "atlanta"; "seattle"; "las-vegas" ])) ])

[<Fact>]
let ``IN with odd whitespace`` () =
    // datadog-api-client-go examples/v1/monitors/CreateMonitor_1303514967.go.
    parses
        "{aws_product IN (amplify ,athena, backup, bedrock ) }"
        (In("aws_product", [ "amplify"; "athena"; "backup"; "bedrock" ]))

[<Fact>]
let ``NOT prefix`` () = parses "{NOT host:a}" (Not(Tag("host", "a")))

[<Fact>]
let ``lower-case keywords`` () =
    parses "{a:1 and not b:2}" (And [ Tag("a", "1"); Not(Tag("b", "2")) ])

[<Fact>]
let ``a keyword prefix is still a tag`` () =
    parses "{ANDROID}" (Bare "ANDROID")

// --- refused --------------------------------------------------------------

[<Fact>]
let ``mixing the two syntaxes is refused`` () =
    fails "{a:1, b:2 AND c:3}" "cannot be mixed"
    fails "{env:prod AND !host:a}" "cannot be mixed"

[<Fact>]
let ``IN after a key:value is refused`` () =
    fails "{env:prod IN (a)}" "bare tag key"

[<Fact>]
let ``unclosed brace`` () = fails "{env:prod" "Expecting"
