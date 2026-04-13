base = MException("MATC:Base", "base boom");
leaf = MException("MATC:Leaf", "leaf boom");
combined = addCause(base, leaf);
kind = class(base);
ok = isa(base, "MException");
throw(combined);
throwAsCaller(combined);
