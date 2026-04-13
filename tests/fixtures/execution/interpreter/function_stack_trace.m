function [caught_id, stack_depth, top_name, bottom_name] = function_stack_trace()
caught_id = "none";
stack_depth = 0;
top_name = "none";
bottom_name = "none";

try
    helper();
catch err
    caught_id = err.identifier;
    stack_depth = numel(err.stack);
    top_name = err.stack(1).name;
    bottom_name = err.stack(end).name;
end

    function helper()
        error("MATC:Nested", "nested boom");
    end
end
