s = struct('a', 1, 'b', 2);
a = structfun(@(x) x + 1, s);
b = structfun(@(x) [x x+1], s, 'UniformOutput', false);
t = struct('a', 1, 'b', 2, 'c', 3);
c = structfun(@structfun_error_demo, t, 'ErrorHandler', @structfun_error_fallback);
[d, e] = structfun(@structfun_pair_demo, t, 'ErrorHandler', @structfun_pair_fallback);

function y = structfun_error_demo(x)
if x == 2
    error('MATC:StructfunFixture', 'boom %d', x);
end
y = x + 30;
end

function y = structfun_error_fallback(err, x)
y = -x - err.index;
end

function [first, second] = structfun_pair_demo(x)
if x == 2
    error('MATC:StructfunPairFixture', 'pair %d', x);
end
first = x;
second = x + 300;
end

function [first, second] = structfun_pair_fallback(err, x)
first = -x;
second = err.index;
end
