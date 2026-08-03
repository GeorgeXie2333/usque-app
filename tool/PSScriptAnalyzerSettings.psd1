# PSScriptAnalyzer settings for Usque tool scripts.
# Warning and Error severities both block (check scripts fail on any finding).
# Fixed module version: PSScriptAnalyzer 1.25.0
@{
    Severity            = @('Error', 'Warning')
    IncludeDefaultRules = $true
}
