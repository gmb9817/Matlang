basic_has_cause = false;
basic_has_stack = false;
report_has_cause = false;
report_has_header = false;
report_has_leaf = false;
report_has_stack = false;
report_kind = "none";

try
    raise_base();
catch err
    basic_has_cause = contains(getReport(err, "basic"), "Caused by:");
    basic_has_stack = contains(getReport(err, "basic"), "Stack:");
    report_has_cause = contains(getReport(err), "Caused by:");
    report_has_header = contains(getReport(err), "MATC:Base: base boom");
    report_has_leaf = contains(getReport(err), "MATC:Leaf: leaf boom");
    report_has_stack = contains(getReport(err), "raise_base");
    report_kind = class(getReport(err));
end

function raise_base()
base = MException("MATC:Base", "base boom");
leaf = MException("MATC:Leaf", "leaf boom");
throw(addCause(base, leaf));
end
