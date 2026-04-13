kind = "none";
message = "none";
report_has_message = false;

err = MException("MATC:Fmt", "value=%d tag=%s", 7, "ok");
kind = class(err);
message = err.message;
report_has_message = contains(getReport(err), "MATC:Fmt: value=7 tag=ok");
