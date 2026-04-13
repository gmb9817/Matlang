function y = handles_and_nested_codegen(x)
f = @inner;
g = @(z) z + x;
a = g(1);
y = inner(a);

function out = inner(step)
out = x + step;
end
end
