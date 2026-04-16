classdef Point < handle
properties
x = 1;
y = 2;
end
methods
function obj = Point(x, y)
obj.x = x;
obj.y = y;
end
function total = total(obj)
total = obj.x + obj.y;
end
end
end
