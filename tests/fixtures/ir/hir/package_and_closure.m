function y = package_and_closure(obj, x)
f = @pkg.helper;
g = @(z) z + x;
a = pkg.helper(x);
b = obj.helper(1);
function out = inner(step)
out = x + step;
end
y = inner(a);
end
