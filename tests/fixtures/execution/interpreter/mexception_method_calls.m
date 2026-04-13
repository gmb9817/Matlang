base = MException("MATC:Base", "value=%d", 5);
cause = MException("MATC:Cause", "cause text");
combined = base.addCause(cause);

report = combined.getReport("basic");
cause_count = numel(combined.cause);

caught = "none";
rethrown = "none";

try
    combined.throw();
catch err
    caught = err.identifier;
end

try
    combined.rethrow();
catch err2
    rethrown = err2.identifier;
end
