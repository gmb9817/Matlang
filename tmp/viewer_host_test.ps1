param([string]$HtmlPath,[string]$ReadyPath)
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.Application]::EnableVisualStyles()
$form = New-Object System.Windows.Forms.Form
$form.Text = 'MATC Host Test'
$form.Width = 1000
$form.Height = 800
$browser = New-Object System.Windows.Forms.WebBrowser
$browser.Dock = [System.Windows.Forms.DockStyle]::Fill
$browser.ScriptErrorsSuppressed = $true
$form.Controls.Add($browser)
$browser.add_DocumentCompleted({
  try {
    $title = $browser.DocumentTitle
    $bodyLen = 0
    if ($browser.Document -and $browser.Document.Body) { $bodyLen = $browser.Document.Body.InnerHtml.Length }
    Set-Content -LiteralPath $ReadyPath -Value ("title=" + $title + "`nbodyLen=" + $bodyLen)
  } catch {
    Set-Content -LiteralPath $ReadyPath -Value $_.Exception.Message
  }
})
$form.Add_Shown({ $browser.Navigate($HtmlPath) })
[void]$form.ShowDialog()
