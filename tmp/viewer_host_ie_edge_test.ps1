param([string]$HtmlPath)
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.Application]::EnableVisualStyles()
$form = New-Object System.Windows.Forms.Form
$form.Text = 'MATC Host IE Edge Test'
$form.Width = 1200
$form.Height = 900
$browser = New-Object System.Windows.Forms.WebBrowser
$browser.Dock = [System.Windows.Forms.DockStyle]::Fill
$browser.ScriptErrorsSuppressed = $true
$form.Controls.Add($browser)
$form.Add_Shown({ $browser.Navigate($HtmlPath) })
[void]$form.ShowDialog()
