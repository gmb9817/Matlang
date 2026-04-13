function y = outer(x)
sum = x;
helper = x;
a = helper;
b = helper(1);
c = sum(1);
d = @helper;
e = @sum;
f = zeros(1, 2);
g = localonly(1);
y = a + b + c;
function z = helper(v)
z = v + 1;
end
end

function z = localonly(v)
z = v;
end
