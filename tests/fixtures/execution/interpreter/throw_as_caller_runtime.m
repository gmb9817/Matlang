function out = throw_as_caller_runtime()
try
    helper();
    out = struct("status", "missed");
catch err
    out = struct();
    out.identifier = err.identifier;
    out.stack_depth = numel(err.stack);
    out.top_name = err.stack(1).name;
    out.cause_count = numel(err.cause);
end
end

function helper()
base = MException("MATC:Caller", "caller boom");
cause = MException("MATC:Cause", "cause boom");
throwAsCaller(addCause(base, cause));
end
