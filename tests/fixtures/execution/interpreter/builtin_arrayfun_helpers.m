a = arrayfun(@(x) x + 1, [1 2 3]);
b = arrayfun(@plus, [1 2; 3 4], [10 20; 30 40]);
s = struct('x', {1, 2});
c = arrayfun(@(item) item.x + 1, s);
d = arrayfun(@(x) [x x+1], [1 2], 'UniformOutput', false);
e = arrayfun(@(x) string(x), [1 0 2]);
f = arrayfun(@arrayfun_error_demo, [1 2 3], 'ErrorHandler', @arrayfun_error_fallback);
[g, h] = arrayfun(@arrayfun_pair_demo, [1 2 3], 'ErrorHandler', @arrayfun_pair_fallback);

function y = arrayfun_error_demo(x)
if x == 2
    error('MATC:ArrayfunFixture', 'boom %d', x);
end
y = x + 10;
end

function y = arrayfun_error_fallback(err, x)
y = -x - err.index;
end

function [first, second] = arrayfun_pair_demo(x)
if x == 2
    error('MATC:ArrayfunPairFixture', 'pair %d', x);
end
first = x;
second = x + 100;
end

function [first, second] = arrayfun_pair_fallback(err, x)
first = -x;
second = err.index;
end
