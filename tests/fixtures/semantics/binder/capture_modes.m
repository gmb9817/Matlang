function y = outer(a)
x = a;
f = @(z) x + z;
inner(1);
y = x;
function inner(step)
x = x + step;
t = @(w) w + x;
end
end
