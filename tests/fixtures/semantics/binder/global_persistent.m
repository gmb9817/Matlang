function y = outer(step)
global g;
persistent p;
g = step;
p = step;
function inner(delta)
global g;
g = g + delta;
p = p + delta;
end
inner(1);
y = g + p;
end
