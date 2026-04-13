function y = function_file_nested_capture(n)
x = 0;
for i = 1:n
    inner(i);
end
y = x;
    function inner(step)
        x = x + step;
    end
end
