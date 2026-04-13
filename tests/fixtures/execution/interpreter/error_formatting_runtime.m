explicit_id = "none";
explicit_msg = "none";
implicit_id = "none";
implicit_msg = "none";

try
    error("MATC:Fmt", "value=%d tag=%s", 7, "ok");
catch err
    explicit_id = err.identifier;
    explicit_msg = err.message;
end

try
    error("fallback %.1f %s", 2.5, "units");
catch err
    implicit_id = err.identifier;
    implicit_msg = err.message;
end
