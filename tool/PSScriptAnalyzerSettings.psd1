# PSScriptAnalyzer settings for Usque tool scripts.
# Warning and Error severities block. PSUseCorrectCasing emits Information, so
# check_source.ps1 and CI run that selected rule separately and block its findings.
# Fixed module version: PSScriptAnalyzer 1.25.0
@{
    Severity            = @('Error', 'Warning')
    IncludeDefaultRules = $true
    Rules               = @{
        PSUseConsistentIndentation = @{
            Enable              = $true
            Kind                = 'space'
            PipelineIndentation = 'IncreaseIndentationForFirstPipeline'
            IndentationSize     = 4
        }
        PSUseConsistentWhitespace  = @{
            Enable                                  = $true
            CheckInnerBrace                         = $true
            CheckOpenBrace                          = $true
            CheckOpenParen                          = $true
            CheckOperator                           = $true
            CheckPipe                               = $true
            CheckPipeForRedundantWhitespace         = $false
            CheckSeparator                          = $true
            CheckParameter                          = $false
            IgnoreAssignmentOperatorInsideHashTable = $true
        }
        PSUseCorrectCasing         = @{
            Enable = $true
        }
    }
}
