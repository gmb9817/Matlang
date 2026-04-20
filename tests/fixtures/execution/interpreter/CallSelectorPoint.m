classdef CallSelectorPoint
properties
x = 0;
end
methods
function obj = CallSelectorPoint(x)
obj.x = x;
end
function total = total(obj)
total = obj.x;
end
end
end
