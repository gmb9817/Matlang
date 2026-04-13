inner_id = "none";
inner_top = "none";
outer_depth = 0;
outer_id = "none";
outer_msg = "none";
outer_top = "none";
status = "none";

try
    try
        error("MATC:Inner", "inner boom");
    catch err
        inner_id = err.identifier;
        inner_top = err.stack(1).name;
        rethrow(err);
    end
catch outer
    outer_depth = numel(outer.stack);
    outer_id = outer.identifier;
    outer_msg = outer.message;
    outer_top = outer.stack(1).name;
    status = "rethrown";
end
