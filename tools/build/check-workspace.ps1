$cargo = Get-Command cargo -ErrorAction SilentlyContinue

if (-not $cargo) {
    Write-Error "cargo is not installed or not available on PATH. Install Rust, then rerun this script."
    exit 1
}

& $cargo.Source check --workspace
exit $LASTEXITCODE
