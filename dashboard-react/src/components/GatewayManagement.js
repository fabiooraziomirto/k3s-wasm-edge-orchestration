import React, { useState, useEffect } from 'react';
import {
  Table,
  Card,
  Button,
  Space,
  Tag,
  Modal,
  Form,
  Input,
  Select,
  Popconfirm,
  Typography,
  Row,
  Col,
  Statistic,
  Descriptions,
  Switch,
  Tooltip,
} from 'antd';
import {
  PlusOutlined,
  DeleteOutlined,
  ReloadOutlined,
  CheckCircleOutlined,
  ExclamationCircleOutlined,
  ClockCircleOutlined,
  SettingOutlined,
  QuestionCircleOutlined,
} from '@ant-design/icons';
import { apiGet, apiPost, apiDelete, apiPut } from '../utils/api';
import { App } from 'antd';

const { Title } = Typography;
const { Option } = Select;

const GatewayManagement = () => {
  const { message } = App.useApp();
  const [gateways, setGateways] = useState([]);
  const [loading, setLoading] = useState(false);
  const [modalVisible, setModalVisible] = useState(false);
  const [configModalVisible, setConfigModalVisible] = useState(false);
  const [selectedGateway, setSelectedGateway] = useState(null);
  const [form] = Form.useForm();
  const [configForm] = Form.useForm();

  // Initialize data only once
  useEffect(() => {
    fetchGateways();
  }, []);

  const fetchGateways = async () => {
    setLoading(true);
    try {
      const data = await apiGet('/api/v1/gateways', 10000);
      let gatewayList = data.gateways || [];
      
      // Use real data from backend - no mock data
      setGateways(gatewayList);
    } catch (error) {
      console.error('Error fetching gateways:', error);
    } finally {
      setLoading(false);
    }
  };

  const handleCreateGateway = async (values) => {
    try {
      // Use form values or auto-generate if empty
      const gatewayName = values.name || `gateway-${gateways.length + 1}`;
      // Don't set endpoint - let the gateway controller set it automatically to Kubernetes service DNS
      // The endpoint will be set to {gateway-name}-service.wasmbed.svc.cluster.local:8080 by the controller
      
      const result = await apiPost('/api/v1/gateways', {
        name: gatewayName,
        // endpoint will be set automatically by gateway controller
        description: `Gateway created from dashboard: ${gatewayName}`
      }, 15000);
      
      if (result.success) {
        message.success(result.message || 'Gateway created successfully');
      setModalVisible(false);
      form.resetFields();
      fetchGateways(); // Refresh the list
      } else {
        message.error(result.message || 'Failed to create gateway');
      }
    } catch (error) {
      console.error('Error creating gateway:', error);
      const errorMessage = error.message || error.toString() || 'Unknown error';
      message.error(`Failed to create gateway: ${errorMessage}`);
    }
  };

  const handleDeleteGateway = async (gatewayId) => {
    try {
      await apiDelete(`/api/v1/gateways/${gatewayId}`, 10000);
      setGateways(prevGateways => prevGateways.filter(gateway => gateway.id !== gatewayId));
      console.log('Gateway deleted successfully:', gatewayId);
    } catch (error) {
      console.error('Error deleting gateway:', error);
    }
  };

  const handleUpdateGatewayConfig = async (values) => {
    try {
      const updatedGateway = await apiPut(`/api/v1/gateways/${selectedGateway.id}`, values, 15000);
      
      setGateways(prevGateways => 
        prevGateways.map(gateway => 
          gateway.id === selectedGateway.id 
            ? updatedGateway
            : gateway
        )
      );
      console.log('Gateway configuration updated successfully');
      setConfigModalVisible(false);
      configForm.resetFields();
      setSelectedGateway(null);
    } catch (error) {
      console.error('Error updating gateway configuration:', error);
    }
  };

  const handleToggleGateway = async (gatewayId, enabled) => {
    try {
      const updatedGateway = await apiPost(`/api/v1/gateways/${gatewayId}/toggle`, { enabled }, 10000);
      
      setGateways(prevGateways => 
        prevGateways.map(gateway => 
          gateway.id === gatewayId 
            ? updatedGateway
            : gateway
        )
      );
      console.log(`Gateway ${enabled ? 'enabled' : 'disabled'} successfully:`, gatewayId);
    } catch (error) {
      console.error('Error toggling gateway:', error);
    }
  };

  const getStatusTag = (status) => {
    const statusConfig = {
      Running: { color: 'green', icon: <CheckCircleOutlined /> },
      Stopped: { color: 'red', icon: <ExclamationCircleOutlined /> },
      Pending: { color: 'orange', icon: <ClockCircleOutlined /> },
      Failed: { color: 'red', icon: <ExclamationCircleOutlined /> },
    };

    const config = statusConfig[status] || { color: 'default', icon: null };
    return (
      <Tag color={config.color} icon={config.icon}>
        {status}
      </Tag>
    );
  };

  const columns = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      sorter: (a, b) => (a.name || '').localeCompare(b.name || ''),
    },
    {
      title: 'Status',
      dataIndex: 'status',
      key: 'status',
      render: (status) => getStatusTag(status),
      filters: [
        { text: 'Running', value: 'Running', key: 'running' },
        { text: 'Stopped', value: 'Stopped', key: 'stopped' },
        { text: 'Pending', value: 'Pending', key: 'pending' },
        { text: 'Failed', value: 'Failed', key: 'failed' },
      ],
      onFilter: (value, record) => record.status === value,
    },
    {
      title: 'Endpoint',
      dataIndex: 'endpoint',
      key: 'endpoint',
      ellipsis: true,
    },
    {
      title: 'Connected Devices',
      dataIndex: 'connected_devices',
      key: 'connected_devices',
      render: (count) => (
        <Statistic
          value={count || 0}
          valueStyle={{ fontSize: '14px', color: count > 0 ? '#3f8600' : '#999' }}
        />
      ),
    },
    {
      title: 'Last Heartbeat',
      dataIndex: 'lastHeartbeat',
      key: 'lastHeartbeat',
      render: (timestamp) => timestamp ? new Date(timestamp).toLocaleString() : 'Never',
    },
    {
      title: 'Status Control',
      dataIndex: 'status',
      key: 'statusControl',
      render: (status, record) => {
        const isActive = status === 'Running' || status === 'Active';
        return (
          <Switch
            key={`switch-${record.gateway_id || record.id}`}
            checked={isActive}
            onChange={(checked) => handleToggleGateway(record.gateway_id || record.id, checked)}
            checkedChildren="Running"
            unCheckedChildren="Stopped"
            disabled={status === 'Pending'}
          />
        );
      },
    },
    {
      title: 'Actions',
      key: 'actions',
      width: 150,
      fixed: 'right',
      render: (_, record) => (
        <Space size="small"           key={`actions-${record.gateway_id || record.id}`}>
          <Button
            key={`configure-${record.gateway_id || record.id}`}
            type="link"
            icon={<SettingOutlined />}
            onClick={() => {
              setSelectedGateway(record);
              configForm.setFieldsValue({
                heartbeatInterval: record.config?.heartbeatInterval || '30s',
                connectionTimeout: record.config?.connectionTimeout || '10m',
                enrollmentTimeout: record.config?.enrollmentTimeout || '5m',
              });
              setConfigModalVisible(true);
            }}
            size="small"
          >
            Configure
          </Button>
          <Popconfirm
            key={`delete-${record.gateway_id || record.id}`}
            title="Are you sure you want to delete this gateway?"
            onConfirm={() => handleDeleteGateway(record.gateway_id || record.id)}
            okText="Yes"
            cancelText="No"
          >
            <Button 
              type="link" 
              danger 
              icon={<DeleteOutlined />}
              size="small"
            >
              Delete
            </Button>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const gatewayStats = gateways.reduce(
    (acc, gateway) => {
      acc.total++;
      if (gateway.status === 'Running') acc.running++;
      else if (gateway.status === 'Stopped') acc.stopped++;
      else if (gateway.status === 'Pending') acc.pending++;
      else if (gateway.status === 'Failed') acc.failed++;
      acc.totalDevices += gateway.connected_devices || 0;
      return acc;
    },
    { total: 0, running: 0, stopped: 0, pending: 0, failed: 0, totalDevices: 0 }
  );

  return (
    <div>
      <Title level={2}>Gateway Management</Title>
      
      <Row gutter={[16, 16]} style={{ marginBottom: 24 }}>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic
              title="Total Gateways"
              value={gatewayStats.total}
              valueStyle={{ color: '#1890ff' }}
            />
          </Card>
        </Col>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic
              title="Running"
              value={gatewayStats.running}
              valueStyle={{ color: '#3f8600' }}
            />
          </Card>
        </Col>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic
              title="Stopped"
              value={gatewayStats.stopped}
              valueStyle={{ color: '#cf1322' }}
            />
          </Card>
        </Col>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic
              title="Total Devices"
              value={gatewayStats.totalDevices}
              valueStyle={{ color: '#722ed1' }}
            />
          </Card>
        </Col>
      </Row>

      <Card>
        <div style={{ marginBottom: 16 }}>
          <Space>
            <Button
              type="primary"
              icon={<PlusOutlined />}
              onClick={() => setModalVisible(true)}
            >
              Add Gateway
            </Button>
            <Button
              icon={<ReloadOutlined />}
              onClick={fetchGateways}
              loading={loading}
            >
              Refresh
            </Button>
          </Space>
        </div>

        <Table
          columns={columns}
          dataSource={gateways}
          rowKey={(record) => record.gateway_id || record.id}
          loading={loading}
          pagination={{
            pageSize: 10,
            showSizeChanger: true,
            showQuickJumper: true,
            showTotal: (total, range) =>
              `${range[0]}-${range[1]} of ${total} gateways`,
          }}
          scroll={{ x: 'max-content' }}
          size="small"
          expandable={{
            expandedRowRender: (record) => (
              <Descriptions size="small" column={2}>
                <Descriptions.Item label="Capabilities">
                  <Space wrap>
                    {record.capabilities?.map((cap) => (
                      <Tag key={cap} color="blue">{cap}</Tag>
                    ))}
                  </Space>
                </Descriptions.Item>
                <Descriptions.Item label="Heartbeat Interval">
                  {record.config?.heartbeatInterval || '30s'}
                </Descriptions.Item>
                <Descriptions.Item label="Connection Timeout">
                  {record.config?.connectionTimeout || '10m'}
                </Descriptions.Item>
                <Descriptions.Item label="Enrollment Timeout">
                  {record.config?.enrollmentTimeout || '5m'}
                </Descriptions.Item>
                <Descriptions.Item label="Conditions" span={2}>
                  {record.conditions?.length > 0 ? (
                    <Space direction="vertical" size="small">
                      {record.conditions.map((condition, index) => (
                        <div key={index}>
                          <Tag color={condition.status === 'True' ? 'green' : 'red'}>
                            {condition.type}: {condition.status}
                          </Tag>
                          {condition.message && (
                            <div style={{ fontSize: '12px', color: '#666' }}>
                              {condition.message}
                            </div>
                          )}
                        </div>
                      ))}
                    </Space>
                  ) : 'No conditions'}
                </Descriptions.Item>
              </Descriptions>
            ),
            rowExpandable: (record) => record.capabilities?.length > 0 || record.conditions?.length > 0,
          }}
        />
      </Card>

      <Modal
        title="Create New Gateway"
        open={modalVisible}
        onCancel={() => {
          setModalVisible(false);
          form.resetFields();
        }}
        footer={null}
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={handleCreateGateway}
        >
          <Form.Item
            name="name"
            label="Gateway Name (Optional)"
            help="Leave empty for auto-generated name"
          >
            <Input placeholder="Auto-generated if empty" />
          </Form.Item>

          <Form.Item
            name="endpoint"
            label="Endpoint (Optional)"
            help="Leave empty for auto-generated endpoint"
          >
            <Input placeholder="Auto-generated if empty" />
          </Form.Item>

          <Form.Item>
            <Space>
              <Button type="primary" htmlType="submit">
                Create Gateway
              </Button>
              <Button onClick={() => {
                setModalVisible(false);
                form.resetFields();
              }}>
                Cancel
              </Button>
            </Space>
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={`Configure Gateway: ${selectedGateway?.name}`}
        open={configModalVisible}
        onCancel={() => {
          setConfigModalVisible(false);
          configForm.resetFields();
          setSelectedGateway(null);
        }}
        footer={null}
      >
        <Form
          form={configForm}
          layout="vertical"
          onFinish={handleUpdateGatewayConfig}
        >
          <Form.Item
            name="heartbeatInterval"
            label="Heartbeat Interval"
            rules={[{ required: true, message: 'Please enter heartbeat interval' }]}
          >
            <Select placeholder="Select heartbeat interval">
              <Option value="15s">15 seconds</Option>
              <Option value="30s">30 seconds</Option>
              <Option value="1m">1 minute</Option>
              <Option value="2m">2 minutes</Option>
              <Option value="5m">5 minutes</Option>
            </Select>
          </Form.Item>

          <Form.Item
            name="connectionTimeout"
            label="Connection Timeout"
            rules={[{ required: true, message: 'Please enter connection timeout' }]}
          >
            <Select placeholder="Select connection timeout">
              <Option value="5m">5 minutes</Option>
              <Option value="10m">10 minutes</Option>
              <Option value="15m">15 minutes</Option>
              <Option value="30m">30 minutes</Option>
              <Option value="1h">1 hour</Option>
            </Select>
          </Form.Item>

          <Form.Item
            name="enrollmentTimeout"
            label="Enrollment Timeout"
            rules={[{ required: true, message: 'Please enter enrollment timeout' }]}
          >
            <Select placeholder="Select enrollment timeout">
              <Option value="2m">2 minutes</Option>
              <Option value="5m">5 minutes</Option>
              <Option value="10m">10 minutes</Option>
              <Option value="15m">15 minutes</Option>
              <Option value="30m">30 minutes</Option>
            </Select>
          </Form.Item>

          <Form.Item>
            <Space>
              <Button type="primary" htmlType="submit">
                Update Configuration
              </Button>
              <Button onClick={() => {
                setConfigModalVisible(false);
                configForm.resetFields();
                setSelectedGateway(null);
              }}>
                Cancel
              </Button>
            </Space>
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
};

export default GatewayManagement;
