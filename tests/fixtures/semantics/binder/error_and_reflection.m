kind = class(1);
ok = isa(kind, "char");
try
    error("MATC:Demo", "boom");
catch err
    try
        rethrow(err);
    catch err2
        report = getReport(err2);
        msg = err2.message;
    end
end
