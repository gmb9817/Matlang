s = struct();
s.alpha = 1;
s.beta = [2 3];
s.inner.value = s.beta(2);
s.fn = @helper;
a = s.alpha;
b = s.inner.value;
c = s.fn(4);

function y = helper(x)
y = x + 4;
end
