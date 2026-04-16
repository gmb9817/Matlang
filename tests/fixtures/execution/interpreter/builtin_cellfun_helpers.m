a = cellfun(@(x) x + 1, {1, 2, 3});
b = cellfun(@plus, {1, 2; 3, 4}, {10, 20; 30, 40});
c = cellfun(@(x) [x x+1], {1, 2}, 'UniformOutput', false);
d = cellfun(@(s) string(s), {'a', 'b'});
e = cellfun(@cellfun_error_demo, {1, 2, 3}, 'ErrorHandler', @cellfun_error_fallback);
[f, g] = cellfun(@cellfun_pair_demo, {1, 2, 3}, 'ErrorHandler', @cellfun_pair_fallback);

function y = cellfun_error_demo(x)
if x == 2
    error('MATC:CellfunFixture', 'boom %d', x);
end
y = x + 20;
end

function y = cellfun_error_fallback(err, x)
y = -x - err.index;
end

function [first, second] = cellfun_pair_demo(x)
if x == 2
    error('MATC:CellfunPairFixture', 'pair %d', x);
end
first = x;
second = x + 200;
end

function [first, second] = cellfun_pair_fallback(err, x)
first = -x;
second = err.index;
end
