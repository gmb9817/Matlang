function y = function_file_transitive_nested_capture()
x = 1;
    function set_outer(v)
        function inner()
            x = v;
        end
        inner();
    end
    function out = get_outer()
        out = x;
    end
set_outer(9);
y = get_outer();
end
