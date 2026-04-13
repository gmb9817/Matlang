global g;
g = 4;
a = read_global();
b = bump_global(3);
c = g;
d = persist_seed(5);
e = persist_seed(0);

function y = read_global()
global g;
y = g;
end

function y = bump_global(delta)
global g;
g = g + delta;
y = g;
end

function y = persist_seed(seed)
persistent p;
if seed > 0
    p = seed;
end
y = p;
end
