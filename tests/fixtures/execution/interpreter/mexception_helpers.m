base = MException("MATC:Base", "base boom");
leaf = MException("MATC:Leaf", "leaf boom");
combined = addCause(base, leaf);

base_id = base.identifier;
base_is_mexception = isa(base, "MException");
base_is_struct = isa(base, "struct");
base_kind = class(base);
combined_cause_count = numel(combined.cause);
combined_cause_id = combined.cause{1}.identifier;

try
    throw(combined);
catch err
    caught_id = err.identifier;
    caught_cause_count = numel(err.cause);
    caught_cause_id = err.cause{1}.identifier;
    caught_kind = class(err);
    caught_is_mexception = isa(err, "MException");
end
