function y = function_file_persistent_nested(step)
persistent p;
if isempty(p)
    p = 0;
end
    function inner(delta)
        p = p + delta;
    end
inner(step);
y = p;
end
